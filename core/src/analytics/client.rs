use anyhow::{Context, Result};
use log::error;
use segment::message::{Track, User};
use segment::{AutoBatcher, Batcher, HttpClient};
use serde_json::{Map, Value, json};

use super::event::Event;
use super::session::SessionId;

const APP_ID: &str = "decentraland-launcher-rust";

pub struct AnalyticsClient {
    anonymous_id: String,
    os: String,
    launcher_version: String,
    session_id: SessionId,
    batcher: AutoBatcher,
}

impl AnalyticsClient {
    pub fn new(
        write_key: String,
        anonymous_id: String,
        os: String,
        launcher_version: String,
    ) -> Self {
        let client = HttpClient::default();
        let context = json!({"direct": true});
        let batcher = Batcher::new(Some(context));
        let batcher = AutoBatcher::new(client, batcher, write_key);
        let session_id = SessionId::random();

        Self {
            anonymous_id,
            os,
            launcher_version,
            session_id,
            batcher,
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

        self.batcher.push(msg).await.context("Cannot push")?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        self.batcher.flush().await?;
        Ok(())
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
