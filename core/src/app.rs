use anyhow::{Context, Result};

use crate::analytics::Analytics;
use crate::analytics::event::Event;
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
    analytics: Arc<Mutex<Analytics>>,
}

impl AppState {
    pub async fn setup() -> Result<Self> {
        logs::dispath_logs()?;

        info!("Application setup start. Version: {}", app_version());

        std::panic::set_hook(Box::new(|info| error!("Panic occurred: {:?}", info)));

        Monitoring::try_setup_sentry().context("Cannot setup monitoring")?;

        let mut analytics = analytics::Analytics::new_from_env();
        analytics
            .track_and_flush_silent(Event::LAUNCHER_OPEN {
                version: utils::app_version().to_owned(),
            })
            .await;

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
            Protocol {},
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
