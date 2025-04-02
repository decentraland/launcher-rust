mod event;
mod client;
mod null_client;
mod session;

use client::AnalyticsClient;
use log::{error, info};
use null_client::NullClient;
use session::SessionId;
use event::Event;
use serde_json::Value;
use anyhow::Result;

use crate::utils::get_os_name;

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

    pub fn new_from_env() -> Self {
        let write_key = option_env!("SEGMENT_API_KEY");

        let args: Option<CreateArgs> = match write_key {
            Some(segment_key) => {
                info!("SEGMENT_API_KEY is set successfully from environment variable, segment is available");                
                let anon_id = uuid::Uuid::new_v4().to_string();
                let version = env!("CARGO_PKG_VERSION").to_owned();
                let os_name = get_os_name().to_owned();
                let args = CreateArgs {
                    write_key: segment_key.to_owned(),
                    anonymous_id: anon_id,
                    os: os_name,
                    launcher_version: version,
                };
                Some(args)
            },
            None => {
                error!("SEGMENT_API_KEY is not set to environment variable, segment is not available");
                None
            },
        };
        Analytics::new(args)
    }

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
