use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use log::{error, info};
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
            fingerprint_props: ClientFingerprint::current().into(),
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

    /// Same as [`Self::cleanup`] but with an explicit budget.
    ///
    /// `cleanup` gives the send daemon the crate default of 500ms, which is
    /// plenty for the launcher (the daemon keeps draining the queue for the
    /// lifetime of the process) but not for a short-lived one that exits right
    /// after: a cold DNS lookup plus TLS handshake to Segment rarely fits, and
    /// the event would sit in the persistent queue until the next launcher run
    /// — which never comes for a user who installs and never opens the app.
    pub async fn cleanup_within(&self, timeout: Duration) {
        self.send_daemon
            .wait_until_empty_queue_or_abandon(Some(timeout))
            .await;
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

fn event_data(value: Value) -> Result<Map<String, Value>, String> {
    match value {
        Value::Object(mut map) => match map.remove("data") {
            Some(Value::Object(data)) => Ok(data),
            Some(other) => Err(format!(
                "serialized event data is not a json object: {other:#?}"
            )),
            None => Ok(Map::new()),
        },
        other => Err(format!("serialized event is not an object: {other:#?}")),
    }
}

fn properties_from_event(event: &Event) -> Map<String, Value> {
    match serde_json::to_value(event) {
        Ok(value) => match event_data(value) {
            Ok(map) => map,
            Err(e) => {
                error!("{}", e);
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
        properties.insert("fp_platform".to_owned(), Value::String("override".to_owned()));

        let mut defaults = Map::new();
        defaults.insert(
            "fp_platform".to_owned(),
            Value::String("macos/aarch64".to_owned()),
        );
        defaults.insert("fp_hardware_concurrency".to_owned(), Value::from(8u32));

        merge_static_defaults(&mut properties, &defaults);

        // Caller-supplied value wins.
        assert_eq!(
            properties.get("fp_platform"),
            Some(&Value::String("override".to_owned()))
        );
        // Missing keys are filled in from the defaults.
        assert_eq!(
            properties.get("fp_hardware_concurrency"),
            Some(&Value::from(8u32))
        );
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

    #[test]
    fn event_data_is_empty_for_unit_variant_event() -> Result<()> {
        let value = serde_json::to_value(&Event::FETCH_VERSION_START)?;
        let data = event_data(value).map_err(|e| anyhow!(e))?;
        assert_eq!(data, Map::new());
        Ok(())
    }

    #[test]
    fn event_data_extracts_fields_for_data_carrying_variant() -> Result<()> {
        let value = serde_json::to_value(&Event::FETCH_VERSION_SUCCESS {
            version: "1.0".to_owned(),
        })?;
        let data = event_data(value).map_err(|e| anyhow!(e))?;
        assert_eq!(data.get("version"), Some(&Value::String("1.0".to_owned())));
        Ok(())
    }

    #[test]
    fn event_data_errors_when_data_is_not_an_object() {
        let value = json!({"event": "custom", "data": "not-an-object"});
        assert!(event_data(value).is_err());
    }
}
