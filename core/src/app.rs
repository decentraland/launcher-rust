use anyhow::{Context, Result};

use crate::analytics::event::Event;
use crate::analytics::Analytics;
use crate::flow::{LaunchFlow, LaunchFlowState};
use crate::{analytics, logs, utils};
use crate::installs;
use crate::monitoring::Monitoring;
use std::sync::Arc;
use tokio::sync::Mutex;
use log::{error, info};

pub struct AppState {
    pub flow: LaunchFlow,
    pub state: Arc<Mutex<LaunchFlowState>>,
    analytics: Arc<Mutex<Analytics>>,
}

impl AppState {
    pub async fn setup() -> Result<Self> {
        logs::dispath_logs()?;

        info!("Application setup start");

        std::panic::set_hook(Box::new(|info| error!("Panic occurred: {:?}", info)));

        Monitoring::try_setup_sentry().context("Cannot setup monitoring")?;

        let mut analytics = analytics::Analytics::new_from_env(); 
        analytics.track_and_flush(Event::LAUNCHER_OPEN { version: utils::app_version().to_owned() })
            .await
            .context("Cannot flush open event")?; 

        let analytics = Arc::new(Mutex::new(analytics));
        let installs_hub = Arc::new(Mutex::new(installs::InstallsHub::new(analytics.clone())));

        let flow = LaunchFlow::new(installs_hub, analytics.clone());
        let flow_state = LaunchFlowState::default();
        let app_state = AppState {
            flow,
            state: Arc::new(Mutex::new(flow_state)),
            analytics
        };

        info!("Application setup complete");

        Ok(app_state)
    }

    pub async fn cleanup(&self) {
        let mut guard = self.analytics.lock().await;
        let result = guard.track_and_flush(Event::LAUNCHER_CLOSE { version: utils::app_version().to_owned() }).await; 
        if let Err(e) = result {
            error!("Cannot flush launcher close event {}", e);
        }
    }
}
