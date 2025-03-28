mod event;
mod client;
mod null_client;
mod session;

use client::AnalyticsClient;
use null_client::NullClient;
use session::SessionId;
use event::Event;
use serde_json::Value;
use anyhow::Result;

pub struct CreateArgs {
    write_key: String, 
    anonymous_id: String, 
    os: String, 
    launcher_version: String,
}

pub enum Analytics {
    Client(AnalyticsClient),
    Null(NullClient),
}

impl Analytics {

    pub fn new(args: Option<CreateArgs>) -> Self {
        match args {
            Some(a) => {
                let client = AnalyticsClient::new(a.write_key, a.anonymous_id, a.os, a.launcher_version);
                Analytics::Client(client)
            },
            None => {
                Analytics::Null(NullClient::new())
            },
        }
    }

    pub async fn track_and_flush(&mut self, event: Event, properties: Value) -> Result<()> {
        match self {
            Self::Client(client) => { 
                client.track_and_flush(event, properties).await?;
                Ok(())
            },
            Self::Null(_) => Ok(())
        }
    }

    pub fn anonymous_id(&self) -> &str {
        match self {
            Self::Client(client) => client.anonymous_id(),
            Self::Null(_) => "empty",
        }
    }

    pub fn session_id(&self) -> &SessionId {
        match self {
            Self::Client(client) => client.session_id(),
            Self::Null(client) => client.session_id(),
        }
    }
}
