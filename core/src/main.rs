use anyhow::{Context, Ok, Result};
use dcl_launcher_core::{app::AppState, channel::EventChannel};
use log::info;

struct ConsoleChannel(); 

impl EventChannel for ConsoleChannel {
    fn send(&self, status: dcl_launcher_core::types::Status) -> Result<()> {
        let s = serde_json::to_string_pretty(&status)?;
        info!("{}", s);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app_state = AppState::setup().context("Cannot setup state")?;
    let channel = ConsoleChannel();
    app_state.flow.launch(&channel, app_state.state).await?;
    Ok(())
}
