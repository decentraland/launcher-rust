use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use log::{error, info, warn};
use segment::HttpClient;
use segment::message::{Track, User};
use segment::queue::event_queue::{
    CombinedAnalyticsEventQueue, InMemoryAnalyticsEventQueue, PersistentAnalyticsEventQueue,
};
use segment::queue::event_send_daemon::AnalyticsEventSendDaemon;
use segment::queue::queued_batcher::QueuedBatcher;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use tokio::sync::Mutex;

use crate::analytics::network_info::network_context;
use crate::environment::AppEnvironment;

use super::event::Event;
use super::fingerprint::ClientFingerprint;
use super::session::SessionId;

const APP_ID: &str = "decentraland-launcher-rust";

pub struct AnalyticsClient {
    anonymous_id: String,
    os: String,
    launcher_version: String,
    campaign_anon_user_id: Option<String>,
    session_id: SessionId,
    fingerprint_props: Map<String, Value>,
    batcher: QueuedBatcher,
    send_daemon: AnalyticsEventSendDaemon<HttpClient>,
}

impl AnalyticsClient {
    pub fn new(
        write_key: String,
        anonymous_id: String,
        os: String,
        launcher_version: String,
    ) -> Self {
        let queue = new_event_queue();
        let queue = Arc::new(Mutex::new(queue));

        let context = json!({"direct": true});
        let batcher = QueuedBatcher::new(queue.clone(), Some(context));
        let session_id = SessionId::random();

        let client = HttpClient::default();
        let mut send_daemon = AnalyticsEventSendDaemon::new(queue, None, write_key, client);

        send_daemon.start(|e| error!("{}", e));

        Self {
            anonymous_id,
            os,
            launcher_version,
            campaign_anon_user_id: None,
            session_id,
            fingerprint_props: serialize_fingerprint(&ClientFingerprint::collect()),
            batcher,
            send_daemon,
        }
    }

    pub fn with_campaign_anon_user_id(mut self, id: String) -> Self {
        self.campaign_anon_user_id = Some(id);
        self
    }

    async fn track(&mut self, event: String, mut properties: Map<String, Value>) -> Result<()> {
        properties.insert("os".to_owned(), Value::String(self.os.clone()));
        properties.insert(
            "launcherVersion".to_owned(),
            Value::String(self.launcher_version.clone()),
        );
        properties.insert(
            "sessionId".to_owned(),
            Value::String(self.session_id.value().to_owned()),
        );
        properties.insert("appId".to_owned(), Value::String(APP_ID.to_owned()));

        if let Some(anon_id) = &self.campaign_anon_user_id {
            properties.insert(
                "campaign_anon_user_id".to_owned(),
                Value::String(anon_id.clone()),
            );
        }

        merge_static_defaults(&mut properties, &self.fingerprint_props);

        let user = User::AnonymousId {
            anonymous_id: self.anonymous_id.clone(),
        };

        let properties: Value = Value::Object(properties);
        let context: Option<Value> = Some(network_context());

        let msg = Track {
            user,
            event,
            properties,
            context,
            timestamp: Some(OffsetDateTime::now_utc()),
            ..Default::default()
        };

        match self.batcher.push(msg) {
            Ok(option) => {
                // if something returned then it has not been enqued
                if let Some(msg) = option {
                    self.batcher.flush().await?;
                    if let Err(e) = self.batcher.push(msg) {
                        Err(anyhow!("Cannot push message even after flush: {e}"))
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(anyhow!("Cannot push message to batcher: {e}")),
        }
    }

    async fn flush(&mut self) -> Result<()> {
        self.batcher.flush().await.context("Cannot flush")
    }

    pub async fn track_and_flush(&mut self, event: Event) -> Result<()> {
        let properties = properties_from_event(&event);
        let event_name = format!("{}", event);
        self.track(event_name, properties)
            .await
            .context("Cannot track")?;
        self.flush().await.context("Cannot flush")?;
        Ok(())
    }

    pub const fn anonymous_id(&self) -> &str {
        self.anonymous_id.as_str()
    }

    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub async fn cleanup(&self) {
        self.send_daemon
            .wait_until_empty_queue_or_abandon(None)
            .await;
    }
}

// Serialize the fingerprint snapshot once at client construction time so
// every subsequent `track()` call can merge a pre-built map instead of
// re-serializing the same struct. A serialization failure here would
// indicate a programmer error in `ClientFingerprint` (it's a plain struct
// of primitives), so we degrade to an empty map and log a warning rather
// than failing the analytics client startup.
fn serialize_fingerprint(fp: &ClientFingerprint) -> Map<String, Value> {
    match serde_json::to_value(fp) {
        Ok(Value::Object(map)) => map,
        Ok(other) => {
            warn!("ClientFingerprint serialized to non-object value: {other}");
            Map::new()
        }
        Err(e) => {
            warn!("Cannot serialize ClientFingerprint, dropping fingerprint fields: {e}");
            Map::new()
        }
    }
}

// Per-event properties win over the static defaults so a caller that wants
// to override an individual field (e.g. for a synthetic event) keeps that
// override without having to repeat the rest of the fingerprint.
fn merge_static_defaults(properties: &mut Map<String, Value>, defaults: &Map<String, Value>) {
    for (k, v) in defaults {
        properties.entry(k.clone()).or_insert_with(|| v.clone());
    }
}

fn properties_from_event(event: &Event) -> Map<String, Value> {
    let result = serde_json::to_value(event);
    match result {
        Ok(json) => match json.as_object() {
            Some(map) => match map.get("data") {
                Some(data) => match data {
                    Value::Object(map) => map.to_owned(),
                    _ => {
                        error!("serialized event is not a json object: {:#?}", data);
                        Map::new()
                    }
                },
                None => {
                    error!("serialized event doesn't have data property");
                    Map::new()
                }
            },
            None => {
                error!("serialized event is not an object");
                Map::new()
            }
        },
        Err(error) => {
            error!("Cannot serialize event; {}", error);
            Map::new()
        }
    }
}

fn new_event_queue() -> CombinedAnalyticsEventQueue {
    const DEFAULT_EVENT_COUNT_LIMIT: u32 = 200;

    if AppEnvironment::cmd_args().force_in_memory_analytics_queue {
        info!(
            "CombinedAnalyticsEventQueue created with InMemory queue by flag, InMemoryAnalyticsEventQueue in use"
        );
        return CombinedAnalyticsEventQueue::InMemory(InMemoryAnalyticsEventQueue::new(
            DEFAULT_EVENT_COUNT_LIMIT,
        ));
    }

    let persistent = PersistentAnalyticsEventQueue::new(
        crate::installs::analytics_queue_db_path(),
        DEFAULT_EVENT_COUNT_LIMIT,
    );

    match persistent {
        Ok(persistent) => CombinedAnalyticsEventQueue::Persistent(persistent),
        Err(e) => {
            error!(
                "Cannot create persistent event queue, fallback to InMemory queue: {}",
                e
            );
            CombinedAnalyticsEventQueue::InMemory(InMemoryAnalyticsEventQueue::new(
                DEFAULT_EVENT_COUNT_LIMIT,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_static_defaults_preserves_per_event_properties() {
        let mut properties = Map::new();
        properties.insert("platform".to_owned(), Value::String("override".to_owned()));

        let mut defaults = Map::new();
        defaults.insert("platform".to_owned(), Value::String("macos/aarch64".to_owned()));
        defaults.insert("hardware_concurrency".to_owned(), Value::from(8u32));

        merge_static_defaults(&mut properties, &defaults);

        // Caller-supplied value wins.
        assert_eq!(
            properties.get("platform"),
            Some(&Value::String("override".to_owned()))
        );
        // Missing keys are filled in from the defaults.
        assert_eq!(properties.get("hardware_concurrency"), Some(&Value::from(8u32)));
    }

    #[test]
    fn context_attachments() -> Result<()> {
        let track = Track {
            user: User::AnonymousId {
                anonymous_id: String::new(),
            },
            properties: Value::Null,
            event: "test".to_owned(),
            timestamp: None,
            context: Some(network_context()),
            extra: Map::new(),
            integrations: None,
        };
        let json_value = serde_json::to_value(track.clone())?;

        //TODO strict check
        println!("message: {}", json_value);

        let mut batcher = segment::Batcher::new(Some(json!("{\"type\": \"default context\"}")));
        let _ = batcher.push(track);
        let message = batcher.into_message();
        let json_value = serde_json::to_value(message)?;

        println!("message: {}", json_value);

        Ok(())
    }
}
