mod client;
pub mod event;
mod network_info;
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
    environment::AppEnvironment,
    utils::{app_version, get_os_name},
};

pub struct CreateArgs {
    write_key: String,
    anonymous_id: String,
    os: String,
    launcher_version: String,
}

#[allow(clippy::large_enum_variant)]
pub enum Analytics {
    Client(AnalyticsClient),
    Null(NullClient),
}

impl Analytics {
    pub fn new_from_env() -> Self {
        if AppEnvironment::cmd_args().skip_analytics {
            info!("SEGMENT_API_KEY running with --skip-analytics, segment is not available");
            return Self::new(None);
        }

        let write_key: Option<&str> = option_env!("SEGMENT_API_KEY");

        let args: Option<CreateArgs> = match write_key {
            Some(segment_key) => {
                info!(
                    "SEGMENT_API_KEY is set successfully from environment variable, segment is available"
                );
                let anonymous_id = config::user_id_or_none();
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
        Self::new(args)
    }

    pub fn new(args: Option<CreateArgs>) -> Self {
        match args {
            Some(a) => {
                let client =
                    AnalyticsClient::new(a.write_key, a.anonymous_id, a.os, a.launcher_version);
                Self::Client(client)
            }
            None => Self::Null(NullClient::new()),
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
            error!("Cannot send event: {:#?}", e);
        }
    }

    pub const fn anonymous_id(&self) -> &str {
        match self {
            Self::Client(client) => client.anonymous_id(),
            Self::Null(_) => "empty",
        }
    }

    pub const fn session_id(&self) -> &SessionId {
        match self {
            Self::Client(client) => client.session_id(),
            Self::Null(client) => client.session_id(),
        }
    }

    pub async fn cleanup(&self) {
        if let Self::Client(client) = &self {
            client.cleanup().await;
        }
    }
}
