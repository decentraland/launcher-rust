use std::str::FromStr;

use segment::{HttpClient, AutoBatcher, Batcher};
use segment::message::Track;
use serde_json::{json, Value};
use anyhow::Result;

use super::event::Event;
use super::session::SessionId;

const APP_ID: &str = "decentraland-launcher";

pub struct AnalyticsClient {
    anonymous_id: String,
    os: String,
    launcher_version: String,
    session_id: SessionId,
    batcher: AutoBatcher,
}

impl AnalyticsClient {

    pub fn new(write_key: String, anonymous_id: String, os: String, launcher_version: String) -> Self {
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

    async fn track(&mut self, event: Event, mut properties: Value) -> Result<()> {
        //TODO
        //user real id
        //anon id

        properties["os"] = Value::from_str(&self.os)?;
        properties["launcherVersion"] = Value::from_str(&self.launcher_version)?;
        properties["sessionId"] = Value::from_str(self.session_id.value())?;
        properties["appId"] = Value::from_str(APP_ID)?;

        let msg = Track {
            event: format!("{}", event),
            properties: properties,
            ..Default::default()
        };


        self.batcher.push(msg).await?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<()>{
        self.batcher.flush().await?;
        Ok(())
    }

    pub async fn track_and_flush(&mut self, event: Event, properties: Value) -> Result<()> {
        self.track(event, properties).await?;
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
