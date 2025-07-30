use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use log::error;
use segment::HttpClient;
use segment::message::{Track, User};
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;

use crate::analytics::infrastructure::event_queue::CombinedAnalyticsEventQueue;
use crate::analytics::infrastructure::event_send_daemon::AnalyticsEventSendDaemon;
use crate::analytics::infrastructure::queued_batcher::QueuedBatcher;

use super::event::Event;
use super::session::SessionId;

const APP_ID: &str = "decentraland-launcher-rust";

pub struct AnalyticsClient {
    anonymous_id: String,
    os: String,
    launcher_version: String,
    session_id: SessionId,
    batcher: QueuedBatcher,
    _send_daemon: AnalyticsEventSendDaemon<HttpClient>,
}

impl AnalyticsClient {
    pub fn new(
        write_key: String,
        anonymous_id: String,
        os: String,
        launcher_version: String,
    ) -> Self {
        let queue = CombinedAnalyticsEventQueue::default();
        let queue = Arc::new(Mutex::new(queue));

        let context = json!({"direct": true});
        let batcher = QueuedBatcher::new(queue.clone(), Some(context));
        let session_id = SessionId::random();

        let client = HttpClient::default();
        let mut send_daemon = AnalyticsEventSendDaemon::new(queue, None, write_key, client);

        send_daemon.start();

        Self {
            anonymous_id,
            os,
            launcher_version,
            session_id,
            batcher,
            _send_daemon: send_daemon,
        }
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

        let user = User::AnonymousId {
            anonymous_id: self.anonymous_id.clone(),
        };

        let properties: Value = Value::Object(properties);

        let msg = Track {
            user,
            event,
            properties,
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
        self.batcher.flush().await
    }

    // TODO remove async
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
