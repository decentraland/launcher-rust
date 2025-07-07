use anyhow::anyhow;
use std::{collections::HashMap, fmt::Display};
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

pub type StepResult = std::result::Result<(), StepError>;

pub type StepResultTyped<T> = std::result::Result<T, StepError>;

impl<T> From<StepError> for StepResultTyped<T> {
    fn from(value: StepError) -> Self {
        Self::Err(value)
    }
}

#[allow(non_camel_case_types)]
#[derive(Error, Debug)]
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
}

impl StepError {
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
                    "Internal communication error during download. Please restart the launcher and try again."
                }
            },
            Self::E1001_FILE_NOT_FOUND { .. } => {
                "The downloaded file could not be found. Please try downloading again or check your antivirus and disk permissions."
            }
            Self::E1002_CORRUPTED_ARCHIVE { .. } => {
                "The downloaded file appears to be corrupted. Please try downloading it again."
            }
            Self::E1003_DECOMPRESS_ACCESS_DENIED { .. } => {
                "We couldn’t extract the files. Please run the launcher as administrator or check your folder permissions."
            }
            Self::E1004_DISK_FULL { .. } => {
                "There isn’t enough space on your disk to install Decentraland. Please free up some space and try again."
            }
            Self::E1005_DECOMPRESS_OUT_OF_MEMORY { .. } => {
                "Your system ran out of memory while installing the game. Try closing other programs or restarting your computer."
            }
            Self::E1006_FILE_DELETE_FAILED { .. } => {
                "We couldn’t remove a previous download. Please check your permissions or try restarting the launcher."
            }
            Self::E1007_FILE_CREATE_FAILED { .. } => {
                "We couldn’t create a file to download. Please check your permissions or try restarting the launcher."
            }
            Self::E2001_DOWNLOAD_FAILED { .. } => {
                "There was an error while downloading Decentraland. Please check your internet connection and try again."
            }
            Self::E2002_MISSING_CONTENT_LENGTH { .. } => {
                "Failed to get the file size from the server. Please try again later or verify the download URL is reachable."
            }
            Self::E2003_NETWORK_WRITE_ERROR { .. } => {
                "There was an error while saving the downloaded file. Please make sure you have enough disk space and permission to write to the folder."
            }
            Self::E2004_DOWNLOAD_FAILED_HTTP_CODE { .. } => {
                "There was an error while downloading Decentraland. Please check your internet connection and try again."
            }
            Self::E2005_DOWNLOAD_FAILED_FILE_INCOMPLETE { .. } => {
                "Downloading file is incomplete due an error. Please check your internet connection and try again."
            }
            Self::E2006_DOWNLOAD_FAILED_NETWORK_TIMEOUT => {
                "Timeout while downloading Decentraland. Please check your internet connection and try again."
            }
            Self::E3001_OPEN_DEEPLINK_TIMEOUT => {
                "There was an error while opening the deeplink. Please restart client and try again."
            }
            Self::E3002_PLACE_DEEPLINK_ERROR { .. } => {
                "There was an error while passing the deeplink. Please restart client and try again."
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
