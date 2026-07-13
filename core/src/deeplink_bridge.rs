use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Write;
use std::time::Duration;
use serde::Serialize;
use tokio::fs::remove_file;
use tokio::time::error::Elapsed;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::{
    channel::EventChannel,
    environment::{
        AppEnvironment, Args, ARG_BRIDGE_ONLY, ARG_LOCAL_SCENE, ARG_MULTI_INSTANCE,
        ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE,
    },
    errors::{StepError, StepResult},
    installs::deeplink_bridge_path,
    protocols::DeepLink,
    types::{Status, Step},
};

#[derive(Serialize)]
struct DeepLinkBridgeDTO {
    deeplink: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PlaceDeeplinkError {
    SerializeError,
    IOError,
    Cancelled,
}

impl Display for PlaceDeeplinkError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        use PlaceDeeplinkError::*;
        match self {
            SerializeError => write!(f, "Failed to serialize deeplink data."),
            IOError => write!(f, "Failed to write deeplink to file."),
            Cancelled => write!(f, "Operation was cancelled."),
        }
    }
}

impl From<serde_json::Error> for PlaceDeeplinkError {
    fn from(_: serde_json::Error) -> Self {
        Self::SerializeError
    }
}

impl From<std::io::Error> for PlaceDeeplinkError {
    fn from(_: std::io::Error) -> Self {
        Self::IOError
    }
}

/// How `place_deeplink_and_wait_until_consumed` finished waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeeplinkConsumeOutcome {
    /// The bridge file disappeared, i.e. some process picked up the deeplink.
    Consumed,
    /// Waiting was cancelled before the file was consumed.
    Cancelled,
}

pub type PlaceDeeplinkResult = Result<DeeplinkConsumeOutcome, PlaceDeeplinkError>;

/// Uses `open <path-to-.app>` so Launch Services activates the already-running instance by bundle id.
/// Since the function internally uses "open" command and if instance is not running then it may accidentally open new instance of the app.
/// Keep it in mind using this function.
#[cfg(target_os = "macos")]
fn try_bring_explorer_to_front() {
    let app_path = match crate::installs::get_explorer_launch_path(None) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to resolve Explorer .app path for activation: {e}");
            return;
        }
    };

    let output = std::process::Command::new("open").arg(&app_path).output();

    match output {
        Ok(out) => {
            if out.status.success() {
                log::info!("Finish: Bring Explorer to front at {}", app_path.display());
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                log::warn!(
                    "`open {}` exited with {}: {}",
                    app_path.display(),
                    out.status,
                    stderr.trim()
                );
            }
        }
        Err(e) => {
            log::warn!("Failed to spawn `open` to Bring Explorer to front: {e}");
        }
    }
}

pub fn should_use_deeplink_bridge(
    deeplink: &DeepLink,
    args: &Args,
    any_is_running: bool,
) -> bool {
    let open_new_instance = deeplink.has_true_value(ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE)
        || deeplink.has_true_value(ARG_MULTI_INSTANCE)
        || args.open_new_client_instance;
    let is_local_scene = deeplink.has_true_value(ARG_LOCAL_SCENE) || args.local_scene;
    let bridge_only = deeplink.has_true_value(ARG_BRIDGE_ONLY) || args.bridge_only;

    !open_new_instance && (any_is_running || bridge_only) && !is_local_scene
}

pub async fn should_use_deeplink_bridge_for(
    deeplink: &DeepLink,
    any_is_running: bool,
) -> anyhow::Result<bool> {
    let args = AppEnvironment::cmd_args();
    Ok(should_use_deeplink_bridge(deeplink, &args, any_is_running))
}

pub async fn execute_passthrough<T: EventChannel>(
    channel: &T,
    deeplink: &DeepLink,
) -> StepResult {
    const OPEN_DEEPLINK_TIMEOUT: Duration = Duration::from_secs(3);
    type OpenResult = std::result::Result<PlaceDeeplinkResult, Elapsed>;

    channel.send(Status::State {
        step: Step::DeeplinkOpening,
    })?;

    // In bridge-only mode the deeplink may be consumed by a process the launcher does not manage
    // (e.g. an Explorer launched from the Unity editor). We must not activate the packaged app once
    // the file is consumed, because `open <app>` would launch a spurious new instance when the
    // packaged app is not the consumer. Bringing the window to front is only appropriate for the
    // regular passthrough case, where the running instance is one the launcher itself started.
    let bridge_only = deeplink.has_true_value(ARG_BRIDGE_ONLY) || AppEnvironment::cmd_args().bridge_only;
    let bring_to_front = !bridge_only;

    let token = CancellationToken::new();

    match tokio::time::timeout(
        OPEN_DEEPLINK_TIMEOUT,
        place_deeplink_and_wait_until_consumed(deeplink.clone(), token.child_token()),
    )
    .await
    {
        OpenResult::Ok(result) => match result {
            PlaceDeeplinkResult::Ok(outcome) => {
                // Once the deeplink is placed and consumed, activate the packaged Explorer window,
                // but skip it in bridge-only mode (the consumer owns focus).
                if outcome == DeeplinkConsumeOutcome::Consumed && bring_to_front {
                    #[cfg(target_os = "macos")]
                    try_bring_explorer_to_front();
                }
                StepResult::Ok(())
            }
            PlaceDeeplinkResult::Err(error) => match error {
                PlaceDeeplinkError::SerializeError | PlaceDeeplinkError::IOError => {
                    StepResult::Err(error.into())
                }
                PlaceDeeplinkError::Cancelled => StepResult::Ok(()),
            },
        },
        OpenResult::Err(_) => {
            token.cancel();
            StepResult::Err(StepError::E3001_OPEN_DEEPLINK_TIMEOUT)
        }
    }
}

pub async fn place_deeplink_and_wait_until_consumed(
    deeplink: DeepLink,
    token: CancellationToken,
) -> PlaceDeeplinkResult {
    let path = deeplink_bridge_path();

    // Write deeplink to file
    {
        let dto = DeepLinkBridgeDTO {
            deeplink: deeplink.into(),
        };
        let json = serde_json::to_string(&dto)?;
        File::create(&path)?.write_all(json.as_bytes())?;
    }

    // Wait until file is deleted or operation is cancelled
    loop {
        tokio::select! {
            () = token.cancelled() => {
                // Clean up on cancel
                let _ = remove_file(&path).await;
                return Ok(DeeplinkConsumeOutcome::Cancelled);
            },
            () = sleep(Duration::from_millis(50)) => {
                if !path.exists() {
                    return Ok(DeeplinkConsumeOutcome::Consumed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::Args;
    use crate::protocols::{DeepLink, DeepLinkCreateError};

    fn deeplink(value: &str) -> Result<DeepLink, DeepLinkCreateError> {
        DeepLink::from_string(value.to_string())
    }

    fn args(argv: &[&str]) -> Args {
        Args::parse(argv.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn uses_bridge_when_running_and_no_special_flags() -> Result<(), DeepLinkCreateError> {
        assert!(should_use_deeplink_bridge(
            &deeplink("decentraland://")?,
            &args(&["app"]),
            true
        ));
        Ok(())
    }

    #[test]
    fn no_bridge_when_no_instance_running() -> Result<(), DeepLinkCreateError> {
        assert!(!should_use_deeplink_bridge(
            &deeplink("decentraland://")?,
            &args(&["app"]),
            false
        ));
        Ok(())
    }

    #[test]
    fn no_bridge_when_new_instance_requested_via_deeplink() -> Result<(), DeepLinkCreateError> {
        assert!(!should_use_deeplink_bridge(
            &deeplink(&format!("decentraland://{}=true", ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE))?,
            &args(&["app"]),
            true
        ));
        assert!(!should_use_deeplink_bridge(
            &deeplink(&format!("decentraland://{}=true", ARG_MULTI_INSTANCE))?,
            &args(&["app"]),
            true
        ));
        Ok(())
    }

    #[test]
    fn no_bridge_when_new_instance_requested_via_args() -> Result<(), DeepLinkCreateError> {
        assert!(!should_use_deeplink_bridge(
            &deeplink("decentraland://")?,
            &args(&["app", "--open-deeplink-in-new-instance"]),
            true
        ));
        Ok(())
    }

    #[test]
    fn no_bridge_for_local_scene() -> Result<(), DeepLinkCreateError> {
        assert!(!should_use_deeplink_bridge(
            &deeplink(&format!("decentraland://{}=true", ARG_LOCAL_SCENE))?,
            &args(&["app"]),
            true
        ));
        assert!(!should_use_deeplink_bridge(
            &deeplink("decentraland://")?,
            &args(&["app", "--local-scene"]),
            true
        ));
        Ok(())
    }
}
