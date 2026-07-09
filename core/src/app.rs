use anyhow::{Context, Result};

use crate::analytics::Analytics;
use crate::analytics::event::Event;
use crate::auto_auth::campaign_anon_user_id_storage::CampaignAnonUserIdStorage;
use crate::auto_auth::campaign_attribution_marker::CampaignAttributionMarker;
use crate::flow::{LaunchFlow, LaunchFlowState};
use crate::installs;
use crate::instances::RunningInstances;
use crate::monitoring::Monitoring;
use crate::protocols::Protocol;
use crate::{analytics, logs, utils};
#[cfg(target_os = "macos")]
use crate::auto_auth::AutoAuth;
use log::{error, info};
use std::sync::Arc;
use tokio::sync::Mutex;
use utils::{BUILD_COMMIT, BUILD_PR, app_version};

pub struct AppState {
    pub flow: LaunchFlow,
    pub state: Arc<Mutex<LaunchFlowState>>,
    pub protocol: Protocol,
    pub analytics: Arc<Mutex<Analytics>>,
}

impl AppState {
    pub async fn setup() -> Result<Self> {
        logs::dispath_logs()?;

        info!(
            "Application setup start. Version: {} commit: {} pr: {}",
            app_version(),
            BUILD_COMMIT,
            BUILD_PR
        );

        std::panic::set_hook(Box::new(|info| error!("Panic occurred: {:?}", info)));

        Monitoring::try_setup_sentry().context("Cannot setup monitoring")?;

        #[cfg(target_os = "macos")]
        {
            AutoAuth::try_obtain_auth_token();
            AutoAuth::try_install_to_app_dir_if_from_dmg();
        }

        let campaign_anon_user_id = CampaignAnonUserIdStorage::read();

        let mut analytics = {
            let analytics = analytics::Analytics::new_from_env();
            match &campaign_anon_user_id {
                Some(id) => analytics.with_campaign_anon_user_id(id.as_str()),
                None => analytics,
            }
        };

        analytics
            .track_and_flush_silent(Event::LAUNCHER_OPEN {
                version: utils::app_version().to_owned(),
            })
            .await;

        if let Some(anon_id) = &campaign_anon_user_id {
            if !CampaignAttributionMarker::is_reported() {
                // Mark before sending (at-most-once) to avoid duplicates on crash
                if let Err(e) = CampaignAttributionMarker::mark_reported() {
                    log::warn!("Cannot write attribution marker: {e}");
                }
                info!("Firing Campaign Attribution Detected event");
                analytics
                    .track_and_flush_silent(Event::CAMPAIGN_ATTRIBUTION_DETECTED {
                        anon_user_id: anon_id.as_str().to_owned(),
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

        let flow = LaunchFlow::new(
            installs_hub,
            analytics.clone(),
            running_instances,
        );
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
