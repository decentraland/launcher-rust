use std::cmp::min;
use std::fs::File;
use std::io::Write;
use futures_util::StreamExt;
use reqwest::Client;
    
use crate::channel::EventChannel;
use crate::types::{BuildType, Status, Step};
use anyhow::{Context, Result};
use crate::analytics::event::Event;
use crate::analytics::Analytics;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn download_file<T: EventChannel>(url: &str, path: &str, channel: &T, build_type: &BuildType, analytics: Arc<Mutex<Analytics>>) -> Result<()> {
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

    while let Some(item) = stream.next().await {
        let chunk = item.context(format!("Error while downloading file"))?;
        file.write_all(&chunk)
            .context(format!("Error while writing to file"))?;
        let new = min(downloaded + (chunk.len() as u64), total_size);
        downloaded = new;

        {
            let mut analytics_guard = analytics.lock().await;
            let progress_event = Event::DOWNLOAD_VERSION_PROGRESS {
                downloaded_file_url: url.to_string(),
                size_downloaded: downloaded,
                size_remaining: total_size - downloaded,
            };
            if let Err(e) = analytics_guard.track_and_flush(progress_event).await {
                log::error!("Failed to track download progress event: {}", e);
            }
        }

        let progress: u8 = ((downloaded as f64 / total_size as f64) * 100.0) as u8;
        let event: Status = Status::State { 
            step: Step::Downloading { 
                progress,
                build_type: build_type.clone()
            }
        };
        channel.send(event).context(format!("Cannot send event to channel"))?;
    }

    return Ok(());
}
