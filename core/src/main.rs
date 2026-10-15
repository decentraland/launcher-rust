use anyhow::{Context, Ok, Result};
use dcl_launcher_core::{
    app::{AppState, FlowContext, follow_launch_and_wait_for_outcome},
    channel::EventChannel,
    environment::AppEnvironment,
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
    // A `decentraland://` argument is the launch deeplink, as the Tauri app reads it.
    let args: Vec<String> = AppEnvironment::raw_cmd_args().collect();
    app_state.protocol.try_assign_value_from_vec(&args);
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
