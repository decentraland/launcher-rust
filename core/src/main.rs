use anyhow::{Context, Ok, Result};
use dcl_launcher_core::{app::AppState, channel::EventChannel};
use log::info;

struct ConsoleChannel();

impl EventChannel for ConsoleChannel {
    fn send(&self, status: dcl_launcher_shared::types::Status) -> Result<()> {
        let s = serde_json::to_string_pretty(&status)?;
        info!("{s}");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // This dev CLI is its own "UI" process, so it parses its own argv — the
    // same arguments the Tauri UI would attach to a flow command over IPC.
    let args = dcl_launcher_shared::environment::AppEnvironment::cmd_args();
    let app_state = AppState::setup().context("Cannot setup state")?;
    app_state.activate_analytics(&args).await;
    let channel = ConsoleChannel();
    app_state
        .flow
        .launch(&channel, app_state.state.clone(), args)
        .await
        .map_err(|e| anyhow::anyhow!(e.user_message))
}
