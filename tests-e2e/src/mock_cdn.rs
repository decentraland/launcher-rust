use std::io::Read;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use tiny_http::{Response, Server, StatusCode};

/// Scriptable stand-in for the S3 artifacts bucket.
///
/// The service is pointed at it via the debug-only `DCL_LAUNCHER_BUCKET_URL`
/// override; it answers `.../releases/latest.json` and
/// `.../releases/{version}/Decentraland_{os}.zip`.
pub struct MockCdn {
    server: Arc<Server>,
    state: Arc<Mutex<CdnState>>,
    port: u16,
    worker: Option<JoinHandle<()>>,
}

#[derive(Default)]
pub struct CdnState {
    pub version: String,
    pub zip: Arc<Vec<u8>>,
    /// Force this HTTP status on latest.json requests.
    pub latest_status: Option<u16>,
    /// Force this HTTP status on zip requests.
    pub zip_status: Option<u16>,
    /// Sleep this long between served chunks of the zip (slow download).
    pub zip_chunk_delay: Option<Duration>,
    pub latest_hits: u32,
    pub zip_hits: u32,
    /// Every request seen: method, url, headers.
    pub request_log: Vec<String>,
}

impl MockCdn {
    pub fn start(version: &str, zip: Vec<u8>) -> Result<Self> {
        let server =
            Server::http("127.0.0.1:0").map_err(|e| anyhow!("Cannot start the mock CDN: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .context("Mock CDN has no IP address")?
            .port();

        let server = Arc::new(server);
        let state = Arc::new(Mutex::new(CdnState {
            version: version.to_owned(),
            zip: Arc::new(zip),
            ..CdnState::default()
        }));

        let worker = {
            let server = server.clone();
            let state = state.clone();
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
                    handle(request, &state);
                }
            })
        };

        Ok(Self {
            server,
            state,
            port,
            worker: Some(worker),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Mutates the scripted behavior under the lock.
    pub fn configure(&self, apply: impl FnOnce(&mut CdnState)) {
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        apply(&mut guard);
    }

    pub fn snapshot<R>(&self, read: impl FnOnce(&CdnState) -> R) -> R {
        let guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        read(&guard)
    }
}

impl Drop for MockCdn {
    fn drop(&mut self) {
        self.server.unblock();
        // Never join: the worker may be blocked mid-write to a client that
        // stopped reading; it dies with the test process. Joining here can
        // deadlock the (possibly single-threaded) async runtime that still
        // owns the client side of that socket.
        drop(self.worker.take());
        // Never run tiny_http's Server::drop either: it joins its internal
        // accept thread, and on Windows the accept-wakeup race can hang the
        // test process at exit. Leak the server — the OS reclaims it when
        // the process dies.
        std::mem::forget(self.server.clone());
    }
}

struct ThrottledReader {
    data: Arc<Vec<u8>>,
    pos: usize,
    chunk: usize,
    delay: Duration,
}

impl Read for ThrottledReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let Some(remaining) = self.data.get(self.pos..) else {
            return Ok(0);
        };
        if remaining.is_empty() {
            return Ok(0);
        }
        std::thread::sleep(self.delay);
        let take = remaining.len().min(self.chunk).min(buf.len());
        let Some(source) = remaining.get(..take) else {
            return Ok(0);
        };
        let Some(target) = buf.get_mut(..take) else {
            return Ok(0);
        };
        target.copy_from_slice(source);
        self.pos = self.pos.saturating_add(take);
        Ok(take)
    }
}

fn handle(request: tiny_http::Request, state: &Arc<Mutex<CdnState>>) {
    let url = request.url().to_owned();

    {
        let headers: Vec<String> = request.headers().iter().map(ToString::to_string).collect();
        let entry = format!("{} {} | {}", request.method(), url, headers.join("; "));
        let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
        guard.request_log.push(entry);
    }

    if url.contains("latest.json") {
        let (status, version) = {
            let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
            guard.latest_hits = guard.latest_hits.saturating_add(1);
            (guard.latest_status, guard.version.clone())
        };
        if let Some(code) = status {
            let _ = request.respond(Response::empty(StatusCode(code)));
            return;
        }
        let body = serde_json::json!({ "version": version }).to_string();
        let _ = request.respond(Response::from_string(body));
        return;
    }

    if url.contains(".zip") {
        let (status, zip, delay) = {
            let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
            guard.zip_hits = guard.zip_hits.saturating_add(1);
            (guard.zip_status, guard.zip.clone(), guard.zip_chunk_delay)
        };
        if let Some(code) = status {
            let _ = request.respond(Response::empty(StatusCode(code)));
            return;
        }
        // tiny_http switches to chunked encoding (no Content-Length) above its
        // 32KB threshold, but the launcher's downloader requires a
        // Content-Length — force identity encoding.
        if let Some(delay) = delay {
            let len = zip.len();
            let reader = ThrottledReader {
                data: zip,
                pos: 0,
                chunk: 32 * 1024,
                delay,
            };
            let response = Response::new(StatusCode(200), Vec::new(), reader, Some(len), None)
                .with_chunked_threshold(usize::MAX);
            let _ = request.respond(response);
        } else {
            let response =
                Response::from_data(zip.as_slice().to_vec()).with_chunked_threshold(usize::MAX);
            let _ = request.respond(response);
        }
        return;
    }

    let _ = request.respond(Response::empty(StatusCode(404)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[tokio::test]
    async fn reqwest_sees_the_content_length() -> Result<()> {
        // Must exceed tiny_http's 32KB chunked threshold to prove the
        // identity-encoding override works for real-sized artifacts.
        let cdn = MockCdn::start("v1", vec![0_u8; 4096 * 1024])?;
        let url = format!(
            "{}/@dcl/unity-explorer/releases/v1/Decentraland_windows64.zip",
            cdn.base_url()
        );
        let res = reqwest::Client::new().get(&url).send().await?;
        let headers: Vec<String> = res
            .headers()
            .iter()
            .map(|(k, v)| format!("{k}: {v:?}"))
            .collect();
        let status = res.status();
        let content_length = res.content_length();
        // Consume the body: leaving 4MB unread blocks the mock's writer
        // thread on a full socket buffer.
        let body = res.bytes().await?;
        assert_eq!(
            content_length,
            Some(4096 * 1024),
            "status {status} headers {headers:?}"
        );
        assert_eq!(body.len(), 4096 * 1024);
        Ok(())
    }

    #[test]
    fn zip_response_carries_content_length() -> Result<()> {
        let cdn = MockCdn::start("v1", vec![0_u8; 1024])?;
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", cdn.port))?;
        stream.write_all(
            b"GET /x/releases/v1/Decentraland_windows64.zip HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        let head = String::from_utf8_lossy(&raw);
        let headers: String = head.chars().take(400).collect();
        assert!(
            headers.to_lowercase().contains("content-length: 1024"),
            "raw response head: {headers}"
        );
        Ok(())
    }
}
