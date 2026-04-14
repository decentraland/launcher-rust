use anyhow::{Context, Result};

use crate::analytics::Analytics;
use crate::analytics::event::Event;
use crate::auto_auth::AutoAuth;
use crate::config;
use crate::flow::{LaunchFlow, LaunchFlowState};
use crate::installs;
use crate::instances::RunningInstances;
use crate::monitoring::Monitoring;
use crate::protocols::Protocol;
use crate::{analytics, logs, utils};
use log::{error, info};
use std::sync::Arc;
use tokio::sync::Mutex;
use utils::app_version;

pub struct AppState {
    pub flow: LaunchFlow,
    pub state: Arc<Mutex<LaunchFlowState>>,
    pub protocol: Protocol,
    pub analytics: Arc<Mutex<Analytics>>,
}

impl AppState {
    pub async fn setup() -> Result<Self> {
        logs::dispath_logs()?;

        info!("Application setup start. Version: {}", app_version());

        std::panic::set_hook(Box::new(|info| error!("Panic occurred: {:?}", info)));

        Monitoring::try_setup_sentry().context("Cannot setup monitoring")?;

        AutoAuth::try_obtain_auth_token();
        #[cfg(target_os = "macos")]
        AutoAuth::try_install_to_app_dir_if_from_dmg();

        let mut analytics = analytics::Analytics::new_from_env();

        // Attach campaign anon_user_id if present — stamps all subsequent events
        let campaign_anon_user_id = config::campaign_anon_user_id();
        if let Some(ref anon_id) = campaign_anon_user_id {
            analytics.set_campaign_anon_user_id(anon_id);
        }

        analytics
            .track_and_flush_silent(Event::LAUNCHER_OPEN {
                version: utils::app_version().to_owned(),
            })
            .await;

        // Fire CAMPAIGN_ATTRIBUTION_DETECTED once per install.
        // Uses a marker file (not config) to track at-most-once delivery.
        if let Some(anon_id) = &campaign_anon_user_id {
            let marker = installs::campaign_attribution_reported_path();
            if !marker.exists() {
                // Write marker before sending (at-most-once) to avoid duplicates on crash
                if let Err(e) = std::fs::write(&marker, "") {
                    log::warn!("Cannot write attribution marker: {e}");
                }
                info!("Firing Campaign Attribution Detected event");
                analytics
                    .track_and_flush_silent(Event::CAMPAIGN_ATTRIBUTION_DETECTED {
                        anon_user_id: anon_id.clone(),
                    })
                    .await;
            }
        }

        let analytics = Arc::new(Mutex::new(analytics));
        let running_instances = Arc::new(Mutex::new(RunningInstances::default()));
        let installs_hub = Arc::new(Mutex::new(installs::InstallsHub::new(
            analytics.clone(),
            running_instances.clone(),
        )));

        let flow = LaunchFlow::new(installs_hub, analytics.clone(), running_instances);
        let flow_state = LaunchFlowState::default();
        let app_state = Self {
            flow,
            state: Arc::new(Mutex::new(flow_state)),
            protocol: Protocol {},
            analytics,
        };

        info!("Application setup complete");

        Ok(app_state)
    }

    pub async fn cleanup(&self) {
        let mut analytics = self.analytics.lock().await;
        analytics
            .track_and_flush_silent(Event::LAUNCHER_CLOSE {
                version: utils::app_version().to_owned(),
            })
            .await;
        analytics.cleanup().await;
    }
}
