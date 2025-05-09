use futures_util::StreamExt;
use reqwest::Client;
use std::cmp::min;
use std::fs::File;
use std::io::Write;

use crate::analytics::Analytics;
use crate::analytics::event::Event;
use crate::channel::EventChannel;
use crate::types::{BuildType, Status, Step};
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::Mutex;

async fn track_download_progress(
    analytics: &mut Analytics,
    url: &str,
    downloaded: u64,
    total_size: u64,
) {
    let progress_event = Event::DOWNLOAD_VERSION_PROGRESS {
        downloaded_file_url: url.to_owned(),
        size_downloaded: downloaded,
        size_remaining: total_size - downloaded,
    };
    if let Err(e) = analytics.track_and_flush(progress_event).await {
        log::error!("Failed to track download progress event: {}", e);
    }
}

pub async fn download_file<T: EventChannel>(
    url: &str,
    path: &str,
    channel: &T,
    build_type: &BuildType,
    analytics: Arc<Mutex<Analytics>>,
) -> Result<()> {
    let client = Client::new();

    let res = client
        .get(url)
        .send()
        .await
        .context(format!("Failed to GET from '{}'", &url))?;
    let total_size = res
        .content_length()
        .context(format!("Failed to get content length from '{}'", &url))?;

    let mut file = File::create(path).context(format!("Failed to create file '{}'", path))?;
    let mut downloaded: u64 = 0;
    let mut stream = res.bytes_stream();

    let mut analytics_guard = analytics.lock().await;

    // We don't want to send too many analytics events, so we limit the rate at which we send them.
    let mut last_analytics_time: Option<std::time::Instant> = None;
    let duration = std::time::Duration::from_millis(500);

    while let Some(item) = stream.next().await {
        let chunk = item.context(format!("Error while downloading file"))?;
        file.write_all(&chunk)
            .context(format!("Error while writing to file"))?;
        let new = min(downloaded + (chunk.len() as u64), total_size);
        downloaded = new;

        let should_send = match last_analytics_time {
            None => true,
            Some(last_time) => last_time.elapsed() >= duration,
        };

        if should_send {
            last_analytics_time = Some(std::time::Instant::now());
            track_download_progress(&mut analytics_guard, url, downloaded, total_size).await;
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
            .context(format!("Cannot send event to channel"))?;
    }

    track_download_progress(&mut analytics_guard, url, downloaded, total_size).await;

    return Ok(());
}
