use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Write;
use std::time::Duration;
use tokio::fs::remove_file;
use tokio::time::sleep;

use tokio_util::sync::CancellationToken;

use crate::{installs::deeplink_bridge_path, protocols::DeepLink};
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
        PlaceDeeplinkError::SerializeError
    }
}

impl From<std::io::Error> for PlaceDeeplinkError {
    fn from(_: std::io::Error) -> Self {
        PlaceDeeplinkError::IOError
    }
}

pub type PlaceDeeplinkResult = Result<(), PlaceDeeplinkError>;

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
            _ = token.cancelled() => {
                // Clean up on cancel
                let _ = remove_file(&path).await;
                break;
            },
            _ = sleep(Duration::from_millis(50)) => {
                if !path.exists() {
                    break;
                }
            }
        }
    }

    Ok(())
}
