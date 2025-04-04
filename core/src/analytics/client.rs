use std::str::FromStr;

use log::error;
use segment::{HttpClient, AutoBatcher, Batcher};
use segment::message::{Track, User};
use serde_json::{json, Value};
use anyhow::Result;

use super::event::Event;
use super::session::SessionId;

const APP_ID: &str = "decentraland-launcher";

pub struct AnalyticsClient {
    anonymous_id: String,
    user_id: String,
    os: String,
    launcher_version: String,
    session_id: SessionId,
    batcher: AutoBatcher,
}

impl AnalyticsClient {

    pub fn new(write_key: String, anonymous_id: String, user_id: String, os: String, launcher_version: String) -> Self {
        let client = HttpClient::default();
        let context = json!({"direct": true}); 
        let batcher = Batcher::new(Some(context));
        let batcher = AutoBatcher::new(client, batcher, write_key.to_string());
        let session_id = SessionId::random();

        AnalyticsClient {
            anonymous_id,
            user_id,
            os,
            session_id,
            launcher_version,
            batcher,
        }
    }

    async fn track(&mut self, event: String, mut properties: Value) -> Result<()> {
        properties["os"] = Value::from_str(&self.os)?;
        properties["launcherVersion"] = Value::from_str(&self.launcher_version)?;
        properties["sessionId"] = Value::from_str(self.session_id.value())?;
        properties["appId"] = Value::from_str(APP_ID)?;

        let user = User::Both {
            user_id: self.user_id.clone(),
            anonymous_id: self.anonymous_id.clone()
        };

        let msg = Track {
            user,
            event,
            properties,
            ..Default::default()
        };


        self.batcher.push(msg).await?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<()>{
        self.batcher.flush().await?;
        Ok(())
    }

    pub async fn track_and_flush(&mut self, event: Event) -> Result<()> {
        let properties = properties_from_event(&event);
        let event_name = format!("{}", event);
        self.track(event_name, properties).await?;
        self.flush().await?;
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
        Ok(json) => {
            match json.as_object() {
                Some(map) => {
                    match map.get("data") {
                        Some(data) => {
                            data.to_owned()
                        },
                        None => {
                            error!("serialized event doesn't have data property");
                            json!("{}")
                        },
                    }
                },
                None => {
                    error!("serialized event is not an object");
                    json!("{}")
                },
            }
        },
        Err(error) => {
            error!("Cannot serialize event; {}", error);
            json!("{}")
        },
    }
}
