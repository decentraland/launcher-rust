use anyhow::Result;

use crate::flow::{LaunchFlow, LaunchFlowState};
use crate::analytics;
use crate::installs;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct AppState {
    pub flow: LaunchFlow,
    pub state: Arc<Mutex<LaunchFlowState>>,
}

impl AppState {
    pub fn setup() -> Result<Self> {
        fern::Dispatch::new()
            // Perform allocation-free log formatting
            .format(|out, message, record| {
                out.finish(format_args!(
                        "[{} {} {}] {}",
                        humantime::format_rfc3339(std::time::SystemTime::now()),
                        record.level(),
                        record.target(),
                        message
                ))
            })
        .level(log::LevelFilter::Trace)
            .chain(std::io::stdout())
            .chain(fern::log_file("output.log")?)
            .apply()?;

        //TODO pass real client
        let analytics = Arc::new(Mutex::new(analytics::Analytics::new(None)));

        let installs_hub = Arc::new(Mutex::new(installs::InstallsHub::new(analytics)));

        let flow = LaunchFlow::new(installs_hub);
        let flow_state = LaunchFlowState::default();
        let app_state = AppState {
            flow,
            state: Arc::new(Mutex::new(flow_state)),
        };
        Ok(app_state)
    }
}
