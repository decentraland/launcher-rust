//! Resumable HTTP download.
//!
//! A dropped or stalled connection no longer restarts the whole file: the bytes already on disk
//! stay, and the next request asks only for the rest (`Range: bytes=<have>-`). `If-Range` with the
//! server's validator (`ETag`, or `Last-Modified`) makes sure the rest belongs to the same file; when the
//! file changed, the server sends it whole and the download starts over.
//!
//! A connection that only trickles counts as stalled too: a degraded connection can deliver a few
//! KB/s for a long time while a new one gets full speed.
//!
//! This module only depends on reqwest, tokio and futures-util, so it can be tested on its own
//! against a server that drops and stalls connections (see the tests at the bottom).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, ETAG, HeaderMap, IF_RANGE, LAST_MODIFIED, RANGE};
use reqwest::{Client, RequestBuilder, StatusCode};

#[derive(Debug, Clone)]
pub struct ResumeConfig {
    /// Nothing received for this long counts as a stalled connection; the next attempt continues.
    pub stall_timeout: Duration,
    /// Less than this on average over a whole `stall_timeout` counts as stalled too.
    pub min_bytes_per_second: u64,
    /// Attempts in a row that add no bytes before the download gives up.
    pub max_attempts_without_progress: u32,
    /// Pause before the next attempt, multiplied by the number of attempts without progress.
    pub retry_delay: Duration,
}

impl Default for ResumeConfig {
    fn default() -> Self {
        Self {
            stall_timeout: Duration::from_secs(15),
            min_bytes_per_second: 51_200, // 50 KiB/s
            max_attempts_without_progress: 8,
            retry_delay: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResumableError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The part or its validator could not be created or opened for writing.
    #[error("cannot create {}: {source}", .path.display())]
    CreateFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("server answered {0}")]
    Status(StatusCode),
    #[error("server did not report the file size")]
    NoContentLength,
    #[error("no new bytes in {0} attempts in a row")]
    NoProgress(u32),
}

impl ResumableError {
    /// Worth another attempt: the connection, not the request, went wrong.
    fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::Status(s) => s.is_server_error() || s.as_u16() == 429,
            Self::Io(_) | Self::CreateFile { .. } | Self::NoContentLength | Self::NoProgress(_) => {
                false
            }
        }
    }

    fn create_file(path: &Path, source: std::io::Error) -> Self {
        Self::CreateFile {
            path: path.to_owned(),
            source,
        }
    }
}

/// Creates `part`, or empties it for a download that starts over.
fn create_part(part: &Path) -> Result<File, ResumableError> {
    File::create(part).map_err(|e| ResumableError::create_file(part, e))
}

/// Opens `part` to add the rest of the file.
fn append_part(part: &Path) -> Result<File, ResumableError> {
    OpenOptions::new()
        .append(true)
        .open(part)
        .map_err(|e| ResumableError::create_file(part, e))
}

enum Attempt {
    Complete(u64),
    Interrupted,
}

/// The validator of the bytes in `part`, kept next to it so a later run (the launcher restarted,
/// say) can continue the same file.
fn validator_path(part: &Path) -> PathBuf {
    let mut name = part.as_os_str().to_owned();
    name.push(".validator");
    PathBuf::from(name)
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// `bytes <start>-<end>/<total>` -> (start, total); `bytes */<total>` -> (`u64::MAX`, total).
fn content_range(headers: &HeaderMap) -> Option<(u64, u64)> {
    let value = headers.get(CONTENT_RANGE)?.to_str().ok()?;
    let rest = value.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let total = total.trim().parse().ok()?;
    if range.trim() == "*" {
        return Some((u64::MAX, total));
    }
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    Some((start, total))
}

fn validator_of(headers: &HeaderMap) -> Option<String> {
    let etag = headers.get(ETAG).and_then(|v| v.to_str().ok());
    // If-Range takes only a strong ETag; a weak one cannot vouch for byte ranges.
    let strong = etag.filter(|e| !e.starts_with("W/"));
    strong
        .or_else(|| headers.get(LAST_MODIFIED).and_then(|v| v.to_str().ok()))
        .map(str::to_owned)
}

/// Downloads `url` into `part`, continuing whatever an earlier attempt or run left there.
/// `on_progress(downloaded, total)` runs as bytes arrive. Returns the size of the finished file.
pub async fn download_resumable(
    client: &Client,
    url: &str,
    part: &Path,
    config: &ResumeConfig,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<u64, ResumableError> {
    let validator_file = validator_path(part);
    let mut validator = fs::read_to_string(&validator_file)
        .ok()
        .filter(|v| !v.trim().is_empty());
    let mut attempts_without_progress: u32 = 0;

    loop {
        let have = file_len(part);
        let mut request = client.get(url);
        // Without a validator the bytes on disk may be another file: ask for all of it.
        let resuming = have > 0 && validator.is_some();
        if let (true, Some(v)) = (resuming, &validator) {
            request = request
                .header(RANGE, format!("bytes={have}-"))
                .header(IF_RANGE, v.as_str());
        }

        // The error this attempt ended with, if any: reported if the download gives up now.
        let last_error = match attempt(
            request,
            part,
            have,
            resuming,
            config,
            &mut validator,
            &validator_file,
            &mut on_progress,
        )
        .await
        {
            Ok(Attempt::Complete(total)) => {
                let _ = fs::remove_file(&validator_file);
                return Ok(total);
            }
            Ok(Attempt::Interrupted) => None,
            Err(e) if e.is_retryable() => {
                log::warn!("download attempt failed, continuing: {e}");
                Some(e)
            }
            Err(e) => return Err(e),
        };

        if file_len(part) > have {
            attempts_without_progress = 0;
        } else {
            attempts_without_progress = attempts_without_progress.saturating_add(1);
            if attempts_without_progress >= config.max_attempts_without_progress {
                // A network or server error says more than "no progress"; stalls end as no progress.
                let no_progress = ResumableError::NoProgress(attempts_without_progress);
                return Err(last_error.unwrap_or(no_progress));
            }
        }
        tokio::time::sleep(
            config
                .retry_delay
                .saturating_mul(attempts_without_progress.max(1)),
        )
        .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn attempt(
    request: RequestBuilder,
    part: &Path,
    have: u64,
    resuming: bool,
    config: &ResumeConfig,
    validator: &mut Option<String>,
    validator_file: &Path,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<Attempt, ResumableError> {
    let response = match tokio::time::timeout(config.stall_timeout, request.send()).await {
        // No answer within the stall timeout counts as an interrupted attempt.
        Err(_) => {
            log::warn!("download got no answer within {:?}", config.stall_timeout);
            return Ok(Attempt::Interrupted);
        }
        Ok(sent) => sent?,
    };

    let status = response.status();
    let (mut file, mut written, total) = match status {
        StatusCode::PARTIAL_CONTENT if resuming => match content_range(response.headers()) {
            Some((start, total)) if start == have => (append_part(part)?, have, total),
            // Not the range that was asked for: start over.
            _ => {
                create_part(part)?;
                return Ok(Attempt::Interrupted);
            }
        },
        StatusCode::OK => {
            // The whole file: the first request, a server that ignores ranges, or a changed file.
            let total = response
                .content_length()
                .ok_or(ResumableError::NoContentLength)?;
            *validator = validator_of(response.headers());
            match validator {
                Some(v) => fs::write(validator_file, v)
                    .map_err(|e| ResumableError::create_file(validator_file, e))?,
                None => {
                    let _ = fs::remove_file(validator_file);
                }
            }
            (create_part(part)?, 0, total)
        }
        StatusCode::RANGE_NOT_SATISFIABLE => {
            // Nothing left to send: the part already is the whole file, or it is longer than the file.
            if let Some((_, total)) = content_range(response.headers())
                && have == total
            {
                return Ok(Attempt::Complete(total));
            }
            create_part(part)?;
            return Ok(Attempt::Interrupted);
        }
        other => return Err(ResumableError::Status(other)),
    };

    on_progress(written, total);
    let mut stream = response.bytes_stream();
    // Bytes since `window_start`, for the minimum speed over each `stall_timeout`.
    let mut window_start = Instant::now();
    let mut window_bytes: u64 = 0;
    loop {
        match tokio::time::timeout(config.stall_timeout, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                file.write_all(&chunk)?;
                written = written.saturating_add(chunk.len() as u64);
                window_bytes = window_bytes.saturating_add(chunk.len() as u64);
                on_progress(written.min(total), total);

                let elapsed = window_start.elapsed();
                if elapsed >= config.stall_timeout {
                    let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
                    let floor = config
                        .min_bytes_per_second
                        .saturating_mul(elapsed_ms)
                        .saturating_div(1000);
                    // Trickling: keep what arrived, the next attempt continues on a new connection.
                    if window_bytes < floor {
                        log::warn!(
                            "download too slow at {written} of {total} bytes: {window_bytes} bytes in {elapsed_ms} ms"
                        );
                        file.sync_all()?;
                        return Ok(Attempt::Interrupted);
                    }
                    window_start = Instant::now();
                    window_bytes = 0;
                }
            }
            // Dropped or stalled: keep what arrived, the next attempt continues from there.
            Ok(Some(Err(e))) => {
                log::warn!("download interrupted at {written} of {total} bytes: {e}");
                file.sync_all()?;
                return Ok(Attempt::Interrupted);
            }
            Err(_) => {
                log::warn!("download stalled at {written} of {total} bytes");
                file.sync_all()?;
                return Ok(Attempt::Interrupted);
            }
            Ok(None) => break,
        }
    }
    file.sync_all()?;
    drop(file);

    if written == total {
        Ok(Attempt::Complete(total))
    } else {
        if written > total {
            create_part(part)?;
        }
        Ok(Attempt::Interrupted)
    }
}
