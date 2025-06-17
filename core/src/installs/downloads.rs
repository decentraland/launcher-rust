use futures_util::StreamExt;
use reqwest::Client;
use std::cmp::min;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Write;

use crate::analytics::Analytics;
use crate::analytics::event::Event;
use crate::channel::EventChannel;
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
    FileIncomplete(#[from] FileIncompleteError),
}

impl Display for DownloadFileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadFileError::Generic(e) => write!(f, "Download failed: {}", e),
            DownloadFileError::IO(e) => write!(f, "Download failed due IO error: {}", e),
            DownloadFileError::FileIncomplete(e) => write!(f, "Download failed: {}", e),
        }
    }
}

async fn track_download_progress(
    analytics: Arc<Mutex<Analytics>>,
    url: String,
    downloaded: u64,
    total_size: u64,
) {
    let progress_event = Event::DOWNLOAD_VERSION_PROGRESS {
        downloaded_file_url: url.to_owned(),
        size_downloaded: downloaded,
        size_remaining: total_size - downloaded,
    };
    let mut analytics_guard = analytics.lock().await;
    analytics_guard.track_and_flush_silent(progress_event).await;
}

pub async fn download_file<T: EventChannel>(
    url: &str,
    path: &str,
    channel: &T,
    build_type: &BuildType,
    analytics: Arc<Mutex<Analytics>>,
) -> DownloadFileResult {
    let client = Client::new();

    let res = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to GET from '{}'", &url))?;
    let total_size = res
        .content_length()
        .with_context(|| format!("Failed to get content length from '{}'", &url))?;

    // We don't want to send too many analytics events, so we limit the rate at which we send them.
    let mut last_analytics_time: Option<std::time::Instant> = None;
    let duration = std::time::Duration::from_millis(500);
    let mut tasks = Vec::new();

    let mut downloaded: u64 = 0;
    {
        let mut file = File::create(path).context(format!("Failed to create file '{}'", path))?;
        let mut stream = res.bytes_stream();

        // TODO timeout
        while let Some(item) = stream.next().await {
            let chunk = item.context("Error while downloading file")?;
            file.write_all(&chunk)
                .context("Error while writing to file")?;

            let new = min(downloaded + (chunk.len() as u64), total_size);
            downloaded = new;

            let should_send = match last_analytics_time {
                None => true,
                Some(last_time) => last_time.elapsed() >= duration,
            };

            if should_send {
                last_analytics_time = Some(std::time::Instant::now());
                let task = tokio::spawn(track_download_progress(
                    analytics.clone(),
                    url.to_string(),
                    downloaded,
                    total_size,
                ));
                tasks.push(task);
            }

            let progress: u8 = ((downloaded as f64 / total_size as f64) * 100.0) as u8;
            let event: Status = Status::State {
                step: Step::Downloading {
                    progress,
                    build_type: build_type.clone(),
                },
            };
            channel
                .send(event)
                .context("Cannot send event to channel")?;
        }

        file.sync_all()?;
    }

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

    track_download_progress(analytics, url.to_owned(), downloaded, total_size).await;

    Ok(())
}
