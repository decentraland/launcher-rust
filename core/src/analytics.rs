mod client;
pub mod event;
mod null_client;
mod session;

use anyhow::{Context, Result};
use client::AnalyticsClient;
use event::Event;
use log::{error, info};
use null_client::NullClient;
use session::SessionId;

use crate::{
    config,
    utils::{app_version, get_os_name},
};

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
        if std::env::args().any(|e| e == "--skip-analytics") {
            info!("SEGMENT_API_KEY running with --skip-analytics, segment is not available");
            return Self::new(None);
        }

        let write_key: Option<&str> = option_env!("SEGMENT_API_KEY");

        let args: Option<CreateArgs> = match write_key {
            Some(segment_key) => {
                info!(
                    "SEGMENT_API_KEY is set successfully from environment variable, segment is available"
                );
                let anonymous_id = config::user_id().unwrap_or_else(|e| {
                    error!("Cannot get user id from config, fallback is used: {:#}", e);
                    "none".to_owned()
                });
                let launcher_version = app_version().to_owned();
                let os = get_os_name().to_owned();
                let args = CreateArgs {
                    write_key: segment_key.to_owned(),
                    anonymous_id,
                    os,
                    launcher_version,
                };
                Some(args)
            }
            None => {
                error!(
                    "SEGMENT_API_KEY is not set to environment variable, segment is not available"
                );
                None
            }
        };
        Analytics::new(args)
    }

    pub fn new(args: Option<CreateArgs>) -> Self {
        match args {
            Some(a) => {
                let client =
                    AnalyticsClient::new(a.write_key, a.anonymous_id, a.os, a.launcher_version);
                Analytics::Client(client)
            }
            None => Analytics::Null(NullClient::new()),
        }
    }

    async fn track_and_flush(&mut self, event: Event) -> Result<()> {
        match self {
            Self::Client(client) => {
                client
                    .track_and_flush(event)
                    .await
                    .context("Error on track_and_flush")?;
                Ok(())
            }
            Self::Null(_) => Ok(()),
        }
    }

    pub async fn track_and_flush_silent(&mut self, event: Event) {
        if let Err(e) = self.track_and_flush(event).await {
            error!("Cannot send event: {:#?}", e)
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
