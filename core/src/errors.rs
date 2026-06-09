use anyhow::anyhow;
use std::{collections::HashMap, fmt::Display};
use strum::IntoStaticStr;
use thiserror::Error;

use crate::installs::downloads::{DownloadFileError, FileIncompleteError};

use crate::deeplink_bridge::PlaceDeeplinkError;

use super::types::Status;

pub struct FlowError {
    pub user_message: String,
}

impl From<&FlowError> for Status {
    fn from(err: &FlowError) -> Self {
        Self::Error {
            message: err.user_message.clone(),
        }
    }
}

#[derive(Error, Debug)]
pub struct AttemptError {
    #[source]
    pub(crate) error: StepError,
    pub(crate) attempt: u8,
}

impl Display for AttemptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Error on attempt: {}, cause: {}",
            self.attempt, self.error
        )
    }
}

pub type StepResult = std::result::Result<(), StepError>;

pub type StepResultTyped<T> = std::result::Result<T, StepError>;

impl<T> From<StepError> for StepResultTyped<T> {
    fn from(value: StepError) -> Self {
        Self::Err(value)
    }
}

#[allow(non_camel_case_types)]
#[derive(Error, Debug, IntoStaticStr)]
pub enum StepError {
    E0000_GENERIC_ERROR {
        #[source]
        error: anyhow::Error,
        user_message: Option<String>,
    },

    E1001_FILE_NOT_FOUND {
        expected_path: Option<String>,
    },
    E1002_CORRUPTED_ARCHIVE {
        file_path: String,
        #[source]
        inner_error: anyhow::Error,
    },
    E1003_DECOMPRESS_ACCESS_DENIED {
        #[source]
        inner_error: anyhow::Error,
    },
    E1004_DISK_FULL {},
    E1005_DECOMPRESS_OUT_OF_MEMORY {
        #[source]
        inner_error: anyhow::Error,
    },
    E1006_FILE_DELETE_FAILED {
        file_path: String,
        #[source]
        inner_error: anyhow::Error,
    },
    E1007_FILE_CREATE_FAILED {
        file_path: String,
        #[source]
        source: std::io::Error,
    },

    E2001_DOWNLOAD_FAILED {
        url: Option<String>,
        #[source]
        error: reqwest::Error,
    },
    E2002_MISSING_CONTENT_LENGTH {
        url: String,
        response_headers: HashMap<String, String>,
    },
    E2003_NETWORK_WRITE_ERROR {
        url: String,
        bytes_downloaded: u64,
        destination_path: String,
        inner_error_message: String,
    },
    E2004_DOWNLOAD_FAILED_HTTP_CODE {
        url: String,
        code: u16,
    },
    E2005_DOWNLOAD_FAILED_FILE_INCOMPLETE(#[from] FileIncompleteError),
    E2006_DOWNLOAD_FAILED_NETWORK_TIMEOUT,
    E3001_OPEN_DEEPLINK_TIMEOUT,
    E3002_PLACE_DEEPLINK_ERROR(#[from] PlaceDeeplinkError),
    E3003_CANT_GET_VERSION,
    E3004_CANT_RENAME_LATEST,
    E3005_STALE_BUILD_CLEANUP_FAILED {
        path: String,
        #[source]
        source: std::io::Error,
    },
    E3006_RENAME_BACK_FAILED {
        path: String,
        #[source]
        source: std::io::Error,
    },
    E3007_VERSION_DATA_WRITE_FAILED {
        #[source]
        source: std::io::Error,
    },
    E3008_EXPLORER_ALREADY_RUNNING {
        processes: Vec<String>,
    },
}

impl StepError {
    /// Stable identifier for Sentry grouping. Must not include any variable
    /// data (paths, OS messages) — only the variant name. Sentry fingerprints
    /// off this so all occurrences of the same failure cluster into one issue.
    /// Derived from the variant name via `strum::IntoStaticStr`.
    pub fn code(&self) -> &'static str {
        self.into()
    }

    #[must_use]
    pub fn apply_user_message_if_needed(self, new_user_message: &str) -> Self {
        match self {
            Self::E0000_GENERIC_ERROR {
                error,
                user_message,
            } => match user_message {
                Some(s) => Self::E0000_GENERIC_ERROR {
                    error,
                    user_message: Some(s),
                },
                None => Self::E0000_GENERIC_ERROR {
                    error,
                    user_message: Some(new_user_message.to_owned()),
                },
            },
            e => e,
        }
    }

    // migrate to json config for i18n later
    pub fn user_message(&self) -> &str {
        #[allow(clippy::match_same_arms)]
        match self {
            Self::E0000_GENERIC_ERROR {
                error: _,
                user_message,
            } => match &user_message {
                Some(m) => m,
                None => {
                    "Something went wrong. Please close the launcher and open it again to try once more."
                }
            },
            Self::E1001_FILE_NOT_FOUND { .. } => {
                "We couldn't find the downloaded file. Your antivirus may have removed it. Please add Decentraland as an exception in your antivirus and try again."
            }
            Self::E1002_CORRUPTED_ARCHIVE { .. } => {
                "The download didn't finish correctly. Please try again."
            }
            Self::E1003_DECOMPRESS_ACCESS_DENIED { .. } => {
                "We don't have permission to install Decentraland here. Please right-click the launcher and choose \"Run as administrator\", then try again."
            }
            Self::E1004_DISK_FULL { .. } => {
                "There isn't enough free space on your computer to install Decentraland. Please free up some space and try again."
            }
            Self::E1005_DECOMPRESS_OUT_OF_MEMORY { .. } => {
                "Your computer ran out of memory while installing Decentraland. Please close other programs (or restart your computer) and try again."
            }
            Self::E1006_FILE_DELETE_FAILED { .. } => {
                "We couldn't remove files from a previous download. If Decentraland is open, please close it and try again."
            }
            Self::E1007_FILE_CREATE_FAILED { .. } => {
                "We couldn't save the download to your computer. Please close the launcher and open it again. If the problem continues, try running it as administrator."
            }
            Self::E2001_DOWNLOAD_FAILED { .. } => {
                "The download couldn't finish. Please check your internet connection and try again."
            }
            Self::E2002_MISSING_CONTENT_LENGTH { .. } => {
                "We couldn't start the download. Please check your internet connection and try again in a few minutes."
            }
            Self::E2003_NETWORK_WRITE_ERROR { .. } => {
                "We couldn't save the download to your computer. Please make sure you have enough free space and try again."
            }
            Self::E2004_DOWNLOAD_FAILED_HTTP_CODE { .. } => {
                "The download couldn't finish. Please check your internet connection and try again."
            }
            Self::E2005_DOWNLOAD_FAILED_FILE_INCOMPLETE { .. } => {
                "The download was interrupted. Please check your internet connection and try again."
            }
            Self::E2006_DOWNLOAD_FAILED_NETWORK_TIMEOUT => {
                "The download is taking too long. Please check your internet connection and try again."
            }
            Self::E3001_OPEN_DEEPLINK_TIMEOUT => {
                "We couldn't open the deeplink in Decentraland. Please close Decentraland and try again."
            }
            Self::E3002_PLACE_DEEPLINK_ERROR { .. } => {
                "We couldn't send the deeplink to Decentraland. Please close Decentraland and try again."
            }
            Self::E3003_CANT_GET_VERSION => {
                "We couldn't read your installation details. Please reinstall the launcher to fix this."
            }
            Self::E3004_CANT_RENAME_LATEST => {
                "We couldn't update your Decentraland installation. Please close Decentraland (and pause your antivirus if you have one) and try again. If the problem continues, please reinstall the launcher."
            }
            Self::E3005_STALE_BUILD_CLEANUP_FAILED { .. } => {
                "We couldn't clean up files from a previous version. If Decentraland is open, please close it and try again."
            }
            Self::E3006_RENAME_BACK_FAILED { .. } => {
                "We couldn't prepare the update because a file is in use. If Decentraland is open, please close it and try again."
            }
            Self::E3007_VERSION_DATA_WRITE_FAILED { .. } => {
                "We couldn't save the update details. If Decentraland is open, please close it and try again."
            }
            Self::E3008_EXPLORER_ALREADY_RUNNING { .. } => {
                "Decentraland is already running and is blocking the update. Please close it and try again."
            }
        }
    }
}

impl Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.user_message())
    }
}

impl From<anyhow::Error> for StepError {
    fn from(value: anyhow::Error) -> Self {
        Self::E0000_GENERIC_ERROR {
            error: value,
            user_message: None,
        }
    }
}

impl From<std::io::Error> for StepError {
    fn from(value: std::io::Error) -> Self {
        use std::io::ErrorKind::*;

        match value.kind() {
            OutOfMemory => Self::E1005_DECOMPRESS_OUT_OF_MEMORY {
                inner_error: value.into(),
            },
            NotFound => Self::E1001_FILE_NOT_FOUND {
                expected_path: None,
            },
            PermissionDenied => Self::E1003_DECOMPRESS_ACCESS_DENIED {
                inner_error: value.into(),
            },
            WriteZero | StorageFull => Self::E1004_DISK_FULL {},
            _ => Self::E0000_GENERIC_ERROR {
                error: value.into(),
                user_message: None,
            },
        }
    }
}

impl From<zip::result::ZipError> for StepError {
    fn from(value: zip::result::ZipError) -> Self {
        match value {
            zip::result::ZipError::Io(io_err) => Self::from(io_err),
            zip::result::ZipError::InvalidArchive(msg) => Self::E1002_CORRUPTED_ARCHIVE {
                file_path: String::new(),
                inner_error: anyhow!("Invalid archive: {}", msg),
            },
            zip::result::ZipError::UnsupportedArchive(msg) => Self::E1002_CORRUPTED_ARCHIVE {
                file_path: String::new(),
                inner_error: anyhow!("Unsupported archive: {}", msg),
            },
            zip::result::ZipError::FileNotFound => Self::E1002_CORRUPTED_ARCHIVE {
                file_path: String::new(),
                inner_error: anyhow!("File not found in archive"),
            },
            _ => Self::E0000_GENERIC_ERROR {
                error: anyhow!(value),
                user_message: None,
            },
        }
    }
}

impl From<DownloadFileError> for StepError {
    fn from(value: DownloadFileError) -> Self {
        use DownloadFileError::*;
        match value {
            Generic(e) => e.into(),
            IO(e) => e.into(),
            FileIncomplete(e) => e.into(),
            Network(e) => e.into(),
            ContentLengthNotFound { url } => Self::E2002_MISSING_CONTENT_LENGTH {
                url,
                response_headers: HashMap::new(),
            },
            FileCreateFailed { source, file_path } => {
                Self::E1007_FILE_CREATE_FAILED { file_path, source }
            }
            NetworkTimeout => Self::E2006_DOWNLOAD_FAILED_NETWORK_TIMEOUT,
        }
    }
}

impl From<reqwest::Error> for StepError {
    fn from(value: reqwest::Error) -> Self {
        let url: Option<String> = value.url().map(|e| e.as_str().to_owned());
        Self::E2001_DOWNLOAD_FAILED { url, error: value }
    }
}
