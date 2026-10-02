use reqwest::Client;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::time::Duration;

use crate::analytics::Analytics;
use crate::analytics::event::Event;
use crate::channel::EventChannel;
use crate::installs::resumable::{ResumableError, ResumeConfig, download_resumable};
use crate::types::{BuildType, Status, Step};
use anyhow::Context;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type DownloadFileResult = std::result::Result<(), DownloadFileError>;

#[derive(Debug, thiserror::Error)]
pub struct FileIncompleteError {
    expected_size: u64,
    real_size: u64,
}

impl Display for FileIncompleteError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "file is incomplete: expected size {}, real size {}",
            self.expected_size, self.real_size
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadFileError {
    Generic(#[from] anyhow::Error),
    IO(#[from] std::io::Error),
    Network(#[from] reqwest::Error),

    ContentLengthNotFound {
        url: String,
    },
    FileCreateFailed {
        #[source]
        source: std::io::Error,
        file_path: String,
    },
    FileIncomplete(#[from] FileIncompleteError),
    NetworkTimeout,
    SaveFailed {
        #[source]
        source: std::io::Error,
        file_path: String,
    },
    HttpStatus {
        url: String,
        code: u16,
    },
}

impl Display for DownloadFileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Generic(e) => write!(f, "Download failed: {}", e),
            Self::IO(e) => write!(f, "Download failed due IO error: {}", e),
            Self::Network(e) => write!(f, "Download failed due Network error: {}", e),
            Self::FileIncomplete(e) => write!(f, "Download failed: {}", e),
            Self::ContentLengthNotFound { url } => write!(
                f,
                "Download failed due missing content length from url: {}",
                url
            ),
            Self::FileCreateFailed { source, file_path } => write!(
                f,
                "Download failed due file creation failed: {}, source {}",
                file_path, source
            ),
            Self::NetworkTimeout => write!(f, "Download failed due network timeout"),
            Self::SaveFailed { source, file_path } => write!(
                f,
                "Download failed due file save failed: {}, source {}",
                file_path, source
            ),
            Self::HttpStatus { url, code } => write!(
                f,
                "Download failed due http code {} from url: {}",
                code, url
            ),
        }
    }
}

const PROGRESS_REPORT_STEP: u8 = 25;

async fn track_download_progress(
    analytics: Arc<Mutex<Analytics>>,
    url: String,
    downloaded: u64,
    total_size: u64,
) {
    let progress_event = Event::DOWNLOAD_VERSION_PROGRESS {
        downloaded_file_url: url.clone(),
        size_downloaded: downloaded,
        size_remaining: total_size.saturating_sub(downloaded),
    };
    analytics
        .lock()
        .await
        .track_and_flush_silent(progress_event)
        .await;
}

impl DownloadFileError {
    fn from_resumable(error: ResumableError, url: &str) -> Self {
        match error {
            ResumableError::Network(e) => Self::Network(e),
            ResumableError::Io(e) => Self::IO(e),
            ResumableError::CreateFile { path, source } => Self::FileCreateFailed {
                source,
                file_path: path.display().to_string(),
            },
            ResumableError::NoContentLength => Self::ContentLengthNotFound {
                url: url.to_owned(),
            },
            ResumableError::NoProgress(_) => Self::NetworkTimeout,
            ResumableError::Status(code) => Self::HttpStatus {
                url: url.to_owned(),
                code: code.as_u16(),
            },
        }
    }
}

/// Downloads `url` to `path`, continuing after dropped or stalled connections.
///
/// The bytes go to `<path>.part` first and survive a dropped or stalled connection, a failed
/// attempt and a restarted launcher: the next try continues them with a Range request instead of
/// starting the whole file over (see `resumable.rs`).
#[allow(clippy::future_not_send)]
pub async fn download_file<T: EventChannel>(
    url: &str,
    path: &str,
    channel: &T,
    build_type: &BuildType,
    analytics: Arc<Mutex<Analytics>>,
) -> DownloadFileResult {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()?;
    let part = PathBuf::from(format!("{path}.part"));

    // We don't want to send too many analytics events, so we only report every
    // PROGRESS_REPORT_STEP percent — at most 4 events per download.
    let mut next_report_at: u8 = PROGRESS_REPORT_STEP;
    let mut tasks = Vec::new();
    let mut channel_error = None;

    let total_size = download_resumable(
        &client,
        url,
        &part,
        &ResumeConfig::default(),
        |downloaded, total_size| {
            #[allow(
                clippy::arithmetic_side_effects,
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let progress: u8 = ((downloaded as f64 / total_size as f64) * 100.0) as u8;

            if next_report_at < 100 && progress >= next_report_at {
                next_report_at = next_report_at.saturating_add(PROGRESS_REPORT_STEP);
                tasks.push(tokio::spawn(track_download_progress(
                    analytics.clone(),
                    url.to_string(),
                    downloaded,
                    total_size,
                )));
            }

            let event = Status::from(Step::Downloading {
                progress,
                build_type: build_type.clone(),
            });
            if let Err(e) = channel.send(event) {
                channel_error.get_or_insert(e);
            }
        },
    )
    .await
    .map_err(|e| DownloadFileError::from_resumable(e, url))?;

    if let Some(e) = channel_error {
        return Err(e.context("Cannot send event to channel").into());
    }

    std::fs::rename(&part, path).map_err(|source| DownloadFileError::SaveFailed {
        source,
        file_path: path.to_owned(),
    })?;
    let metadata = std::fs::metadata(path)?;
    if metadata.len() != total_size {
        return Err(FileIncompleteError {
            expected_size: total_size,
            real_size: metadata.len(),
        }
        .into());
    }

    for task in tasks {
        task.await.context("Failed to await analytics task")?;
    }

    track_download_progress(analytics, url.to_owned(), total_size, total_size).await;

    Ok(())
}
