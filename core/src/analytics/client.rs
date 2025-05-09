use std::str::FromStr;

use anyhow::{Context, Result};
use log::error;
use segment::message::{Track, User};
use segment::{AutoBatcher, Batcher, HttpClient};
use serde_json::{Value, json};

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
        let batcher = AutoBatcher::new(client, batcher, write_key.to_string());
        let session_id = SessionId::random();

        AnalyticsClient {
            anonymous_id,
            os,
            session_id,
            launcher_version,
            batcher,
        }
    }

    async fn track(&mut self, event: String, mut properties: Value) -> Result<()> {
        properties["os"] = Value::String(self.os.clone());
        properties["launcherVersion"] = Value::String(self.launcher_version.clone());
        properties["sessionId"] = Value::String(self.session_id.value().to_owned());
        properties["appId"] =Value::String(APP_ID.to_owned());

        let user = User::AnonymousId {
            anonymous_id: self.anonymous_id.clone(),
        };

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

    pub fn anonymous_id(&self) -> &str {
        self.anonymous_id.as_str()
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }
}

fn properties_from_event(event: &Event) -> Value {
    let result = serde_json::to_value(event);
    match result {
        Ok(json) => match json.as_object() {
            Some(map) => match map.get("data") {
                Some(data) => data.to_owned(),
                None => {
                    error!("serialized event doesn't have data property");
                    json!("{}")
                }
            },
            None => {
                error!("serialized event is not an object");
                json!("{}")
            }
        },
        Err(error) => {
            error!("Cannot serialize event; {}", error);
            json!("{}")
        }
    }
}
