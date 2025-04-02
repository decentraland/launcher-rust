use anyhow::Result;

use crate::flow::{LaunchFlow, LaunchFlowState};
use crate::{analytics, logs};
use crate::installs;
use crate::monitoring::Monitoring;
use std::sync::Arc;
use tokio::sync::Mutex;
use log::info;

pub struct AppState {
    pub flow: LaunchFlow,
    pub state: Arc<Mutex<LaunchFlowState>>,
}

impl AppState {
    pub fn setup() -> Result<Self> {
        logs::dispath_logs()?;

        info!("Application setup start");

        Monitoring::try_setup_sentry()?;

        //TODO pass real client
        let analytics = Arc::new(Mutex::new(analytics::Analytics::new(None)));

        let installs_hub = Arc::new(Mutex::new(installs::InstallsHub::new(analytics)));

        let flow = LaunchFlow::new(installs_hub);
        let flow_state = LaunchFlowState::default();
        let app_state = AppState {
            flow,
            state: Arc::new(Mutex::new(flow_state)),
        };

        info!("Application setup complete");
        Ok(app_state)
    }
}
