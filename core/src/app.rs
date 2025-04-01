use anyhow::Result;

use crate::flow::{LaunchFlow, LaunchFlowState};
use crate::analytics;
use crate::installs;
use std::sync::Arc;
use tokio::sync::Mutex;
use log::info;

pub struct AppState {
    pub flow: LaunchFlow,
    pub state: Arc<Mutex<LaunchFlowState>>,
}

impl AppState {
    pub fn setup() -> Result<Self> {
        AppState::dispath_logs()?;

        info!("Application setup start");
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

    fn dispath_logs() -> Result<()> {
        let path = installs::log_file_path()?;
        let log_file = fern::log_file(&path)?;
        let path = path.to_string_lossy().to_string();
        println!("Write logs to path: {}", &path);

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
            .chain(log_file)
            .apply()?;

        info!("Logs setup to path: {}", &path);
        Ok(())
    }
}
