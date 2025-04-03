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

use crate::{config, utils::{ app_version, get_os_name}};

pub struct CreateArgs {
    write_key: String, 
    anonymous_id: String, 
    user_id: String,
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
                let anonymous_id = uuid::Uuid::new_v4().to_string();
                let user_id = config::user_id().unwrap_or_else(|e| {
                    error!("Cannot get user id from config, fallback is used: {}", e);
                    "none".to_owned()
                });
                let launcher_version = app_version().to_owned();
                let os = get_os_name().to_owned();
                let args = CreateArgs {
                    write_key: segment_key.to_owned(),
                    anonymous_id,
                    user_id,
                    os,
                    launcher_version,
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
                let client = AnalyticsClient::new(a.write_key, a.anonymous_id, a.user_id, a.os, a.launcher_version);
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
