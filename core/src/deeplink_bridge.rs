use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Write;
use std::time::Duration;
use tokio::fs::remove_file;
use tokio::time::sleep;

use tokio_util::sync::CancellationToken;

use crate::{
    installs::{deeplink_bridge_path, get_explorer_launch_path},
    protocols::DeepLink,
};
use serde::Serialize;

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

pub type PlaceDeeplinkResult = Result<(), PlaceDeeplinkError>;

/// Best-effort attempt to bring the Explorer window to the front.
///
/// Uses `open <path-to-.app>` so Launch Services activates the already-running
/// instance by bundle id. We avoid `osascript tell application "Decentraland"`
/// because the launcher itself is also named "Decentraland" and that
/// `AppleScript` form resolves by display name, which is ambiguous.
#[cfg(target_os = "macos")]
fn try_bring_explorer_to_front() {
    let app_path = match get_explorer_launch_path(None) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to resolve Explorer .app path for activation: {e}");
            return;
        }
    };

    let output = std::process::Command::new("open").arg(&app_path).output();

    match output {
        Ok(out) if out.status.success() => {
            log::info!("Activated Explorer at {}", app_path.display());
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            log::warn!(
                "`open {}` exited with {}: {}",
                app_path.display(),
                out.status,
                stderr.trim()
            );
        }
        Err(e) => {
            log::warn!("Failed to spawn `open` to activate Explorer: {e}");
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

    // Bring the Explorer window to the front
    #[cfg(target_os = "macos")]
    try_bring_explorer_to_front();

    // Wait until file is deleted or operation is cancelled
    loop {
        tokio::select! {
            () = token.cancelled() => {
                // Clean up on cancel
                let _ = remove_file(&path).await;
                break;
            },
            () = sleep(Duration::from_millis(50)) => {
                if !path.exists() {
                    break;
                }
            }
        }
    }

    Ok(())
}
