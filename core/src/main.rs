use anyhow::{Context, Ok, Result};
use dcl_launcher_core::{
    app::{AppState, FlowContext, follow_launch_and_wait_for_outcome},
    channel::EventChannel,
    fsm::Msg,
    protocols::Protocol,
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
        FlowContext::Launch(runtime) => {
            runtime
                .inbox()
                .send(Msg::LaunchIntentionReceived(Protocol::value()))
                .context("Launch runtime stopped")?;
            follow_launch_and_wait_for_outcome(runtime.watch(), &channel)
                .await
                .map_err(|e| anyhow::anyhow!(e.user_message().to_owned()))
        }
        // The CLI has no window: print the first screen and stop.
        FlowContext::Report(ctx) => ctx.flow.open(&channel, ctx.state.clone()).await,
    }
}
