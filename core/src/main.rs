use anyhow::{Context, Ok, Result};
use dcl_launcher_core::flow::{LaunchIntent, OpenAppInstanceIntent};
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
    let app_state = AppState::setup().await.context("Cannot setup state")?;
    let channel = ConsoleChannel();
    app_state
        .flow
        .launch(
            &channel,
            LaunchIntent::OpenAppInstance(OpenAppInstanceIntent::default()),
            app_state.state,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.user_message))
}
