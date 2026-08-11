use serde::Serialize;
use std::fmt;
use std::fmt::Display;

use crate::errors::AttemptError;

#[allow(non_camel_case_types)]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum Event {
    LAUNCHER_OPEN {
        version: String,
    },
    LAUNCHER_CLOSE {
        version: String,
    },
    FETCH_VERSION_START,
    FETCH_VERSION_SUCCESS {
        version: String,
    },
    FETCH_VERSION_ERROR {
        error: String,
    },
    DOWNLOAD_VERSION {
        version: String,
    },
    DOWNLOAD_VERSION_PROGRESS {
        downloaded_file_url: String,
        size_downloaded: u64,
        size_remaining: u64,
    },
    DOWNLOAD_VERSION_SUCCESS {
        version: String,
    },
    DOWNLOAD_VERSION_ERROR {
        version: Option<String>,
        error: String,
    },
    DOWNLOAD_VERSION_CANCELLED {
        version: String,
    },
    DOWNLOAD_VERSION_SKIPPED {
        version: String,
    },
    INSTALL_VERSION_START {
        version: String,
    },
    INSTALL_VERSION_SUCCESS {
        version: String,
    },
    INSTALL_VERSION_ERROR {
        version: Option<String>,
        error: String,
    },
    INSTALL_VERSION_SKIPPED {
        version: String,
    },
    LAUNCH_CLIENT_START {
        version: String,
    },
    LAUNCH_CLIENT_SUCCESS {
        version: String,
    },
    LAUNCH_CLIENT_ERROR {
        version: String,
        error: String,
    },
    LAUNCHER_UPDATE_CHECKING,
    LAUNCHER_UPDATE_AVAILABLE {
        version: String,
    },
    LAUNCHER_UPDATE_NOT_AVAILABLE,
    LAUNCHER_UPDATE_CANCELLED {
        version: String,
    },
    LAUNCHER_UPDATE_ERROR {
        version: String,
        error: String,
    },
    LAUNCHER_UPDATE_DOWNLOADED {
        version: String,
    },
    FLOW_ATTEMPT_ERROR {
        message: String,
        attempt: u8,
    },
    RETRY_FLOW_BUTTON_CLICK {
        version: String,
    },
    CAMPAIGN_ATTRIBUTION_DETECTED {
        anon_user_id: String,
    },
    LAUNCHER_INSTALLER_START {
        installer_file_name: String,
    },
    LAUNCHER_INSTALLER_FINISH {
        installer_file_name: String,
    },
}

impl Display for Event {
    #[allow(clippy::use_self)]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Event::LAUNCHER_OPEN { .. } => "Launcher Open",
                Event::LAUNCHER_CLOSE { .. } => "Launcher Close",
                Event::FETCH_VERSION_START => "Fetch Version Start",
                Event::FETCH_VERSION_SUCCESS { .. } => "Fetch Version Success",
                Event::FETCH_VERSION_ERROR { .. } => "Fetch Version Error",
                Event::DOWNLOAD_VERSION { .. } => "Download Version",
                Event::DOWNLOAD_VERSION_PROGRESS { .. } => "Download Version Progress",
                Event::DOWNLOAD_VERSION_SUCCESS { .. } => "Download Version Success",
                Event::DOWNLOAD_VERSION_ERROR { .. } => "Download Version Error",
                Event::DOWNLOAD_VERSION_CANCELLED { .. } => "Download Version Cancelled",
                Event::DOWNLOAD_VERSION_SKIPPED { .. } => "Download Version Skipped",
                Event::INSTALL_VERSION_START { .. } => "Install Version Start",
                Event::INSTALL_VERSION_SUCCESS { .. } => "Install Version Success",
                Event::INSTALL_VERSION_ERROR { .. } => "Install Version Error",
                Event::INSTALL_VERSION_SKIPPED { .. } => "Install Version Skipped",
                Event::LAUNCH_CLIENT_START { .. } => "Launch Client Start",
                Event::LAUNCH_CLIENT_SUCCESS { .. } => "Launch Client Success",
                Event::LAUNCH_CLIENT_ERROR { .. } => "Launch Client Error",
                Event::LAUNCHER_UPDATE_CHECKING => "Launcher Update Checking",
                Event::LAUNCHER_UPDATE_AVAILABLE { .. } => "Launcher Update Available",
                Event::LAUNCHER_UPDATE_NOT_AVAILABLE => "Launcher Update Not Available",
                Event::LAUNCHER_UPDATE_CANCELLED { .. } => "Launcher Update Cancelled",
                Event::LAUNCHER_UPDATE_ERROR { .. } => "Launcher Update Error",
                Event::LAUNCHER_UPDATE_DOWNLOADED { .. } => "Launcher Update Downloaded",
                Event::FLOW_ATTEMPT_ERROR { .. } => "Launcher Attempt Error",
                Event::RETRY_FLOW_BUTTON_CLICK { .. } => "Retry Flow Button Click",
                Event::CAMPAIGN_ATTRIBUTION_DETECTED { .. } => "Campaign Attribution Detected",
                Event::LAUNCHER_INSTALLER_START { .. } => "Launcher Installer Start",
                Event::LAUNCHER_INSTALLER_FINISH { .. } => "Launcher Installer Finish",
            }
        )
    }
}

impl From<&AttemptError> for Event {
    fn from(value: &AttemptError) -> Self {
        Self::FLOW_ATTEMPT_ERROR {
            message: value.error.to_string(),
            attempt: value.attempt,
        }
    }
}
