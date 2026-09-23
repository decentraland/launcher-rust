use anyhow::{Context, Ok, Result};
use dcl_launcher_core::{
    app::{AppState, FlowContext},
    channel::EventChannel,
};
use log::info;

struct ConsoleChannel();

impl EventChannel for ConsoleChannel {
    fn send(&self, status: dcl_launcher_core::types::Status) -> Result<()> {
        let s = serde_json::to_string_pretty(&status)?;
        info!("{s}");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app_state = AppState::setup().await.context("Cannot setup state")?;
    let channel = ConsoleChannel();
    match &app_state.context {
        FlowContext::Launch(ctx) => ctx
            .flow
            .launch(&channel, ctx.state.clone())
            .await
            .map_err(|e| anyhow::anyhow!(e.user_message)),
        // The CLI has no window: print the first screen and stop.
        FlowContext::Report(ctx) => ctx.flow.open(&channel, ctx.state.clone()).await,
    }
}
