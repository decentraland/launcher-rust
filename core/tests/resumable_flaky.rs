//! installs/resumable.rs against a local server that drops and stalls connections on purpose.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dcl_launcher_core::installs::resumable::{ResumableError, ResumeConfig, download_resumable};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone, Copy, Debug)]
enum Fault {
    None,
    /// Send this many body bytes, then close the connection.
    DropAfter(usize),
    /// Send this many body bytes, then go silent with the connection open.
    StallAfter(usize),
    /// Send this many body bytes, then the rest 100 bytes every 50 ms (2 KB/s).
    TrickleAfter(usize),
    /// Answer with this status and no body.
    Status(u16),
    /// Send the whole body without a Content-Length.
    NoLength,
}

struct Server {
    data: Vec<u8>,
    etag: String,
    faults: Vec<Fault>, // per response, in order; Fault::None after the list ends
    requests: Vec<Option<u64>>, // the Range start of every request (None = no Range)
    body_bytes_sent: usize,
}

type Shared = Arc<Mutex<Server>>;

fn test_data(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x & 0xff) as u8
        })
        .collect()
}

async fn serve(shared: Shared) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let shared = shared.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    head.extend_from_slice(&buf[..n]);
                }
                let text = String::from_utf8_lossy(&head).to_string();
                let header = |name: &str| {
                    text.lines()
                        .find(|l| {
                            l.to_ascii_lowercase()
                                .starts_with(&format!("{}:", name.to_ascii_lowercase()))
                        })
                        .map(|l| l.split_once(':').unwrap().1.trim().to_string())
                };
                let range_start: Option<u64> = header("range")
                    .and_then(|r| r.strip_prefix("bytes=").map(str::to_owned))
                    .and_then(|r| r.trim_end_matches('-').parse().ok());
                let if_range = header("if-range");

                let (data, etag, fault) = {
                    let mut s = shared.lock().unwrap();
                    s.requests.push(range_start);
                    let fault = if s.faults.is_empty() {
                        Fault::None
                    } else {
                        s.faults.remove(0)
                    };
                    (s.data.clone(), s.etag.clone(), fault)
                };
                if let Fault::Status(code) = fault {
                    let head = format!(
                        "HTTP/1.1 {code} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    return;
                }
                let len = data.len() as u64;
                let honour_range =
                    range_start.is_some() && if_range.as_deref().is_none_or(|v| v == etag);
                let (status, extra, body): (&str, String, &[u8]) = match range_start {
                    Some(start) if honour_range && start >= len => (
                        "416 Range Not Satisfiable",
                        format!("Content-Range: bytes */{len}\r\n"),
                        &[][..],
                    ),
                    Some(start) if honour_range => (
                        "206 Partial Content",
                        format!("Content-Range: bytes {start}-{}/{len}\r\n", len - 1),
                        &data[start as usize..],
                    ),
                    _ => ("200 OK", String::new(), &data[..]),
                };
                let length = match fault {
                    Fault::NoLength => String::new(),
                    _ => format!("Content-Length: {}\r\n", body.len()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\n{length}ETag: {etag}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n{extra}\r\n"
                );
                if socket.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let send = match fault {
                    Fault::None | Fault::NoLength => body.len(),
                    Fault::Status(_) => 0,
                    Fault::DropAfter(n) | Fault::StallAfter(n) | Fault::TrickleAfter(n) => {
                        n.min(body.len())
                    }
                };
                for chunk in body[..send].chunks(16 * 1024) {
                    if socket.write_all(chunk).await.is_err() {
                        return;
                    }
                    shared.lock().unwrap().body_bytes_sent += chunk.len();
                }
                let _ = socket.flush().await;
                if let Fault::TrickleAfter(_) = fault {
                    for chunk in body[send..].chunks(100) {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        if socket.write_all(chunk).await.is_err() || socket.flush().await.is_err() {
                            return;
                        }
                        shared.lock().unwrap().body_bytes_sent += chunk.len();
                    }
                }
                if let Fault::StallAfter(_) = fault {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                // Fault::DropAfter and a finished body: the socket closes here.
            });
        }
    });
    format!("http://{addr}/Decentraland_windows64.zip")
}

fn server(data: Vec<u8>, etag: &str, faults: Vec<Fault>) -> Shared {
    Arc::new(Mutex::new(Server {
        data,
        etag: etag.to_owned(),
        faults,
        requests: Vec::new(),
        body_bytes_sent: 0,
    }))
}

fn part_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("resume-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.zip.part"));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(dir.join(format!("{name}.zip.part.validator")));
    path
}

/// No minimum speed, so only drops and stalls end an attempt; the trickle test sets one.
fn config(stall_ms: u64, max_attempts: u32) -> ResumeConfig {
    ResumeConfig {
        stall_timeout: Duration::from_millis(stall_ms),
        min_bytes_per_second: 0,
        max_attempts_without_progress: max_attempts,
        retry_delay: Duration::from_millis(10),
    }
}

#[tokio::test]
async fn continues_after_dropped_connections() {
    let data = test_data(3_000_000, 1);
    let s = server(data.clone(), "\"v1\"", vec![Fault::DropAfter(700_000); 3]);
    let url = serve(s.clone()).await;
    let part = part_path("drops");
    let total = download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(2000, 5),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(total, 3_000_000);
    assert_eq!(std::fs::read(&part).unwrap(), data);
    let s = s.lock().unwrap();
    // Each attempt asked for the rest, never for the whole file again.
    assert_eq!(
        s.requests,
        vec![None, Some(700_000), Some(1_400_000), Some(2_100_000)]
    );
    assert_eq!(s.body_bytes_sent, 3_000_000);
}

#[tokio::test]
async fn continues_after_stalled_connections() {
    let data = test_data(2_000_000, 2);
    let s = server(
        data.clone(),
        "\"v1\"",
        vec![Fault::StallAfter(500_000), Fault::StallAfter(300_000)],
    );
    let url = serve(s.clone()).await;
    let part = part_path("stalls");
    let total = download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(300, 5),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(total, 2_000_000);
    assert_eq!(std::fs::read(&part).unwrap(), data);
    assert_eq!(
        s.lock().unwrap().requests,
        vec![None, Some(500_000), Some(800_000)]
    );
}

#[tokio::test]
async fn leaves_a_connection_that_only_trickles() {
    let data = test_data(1_000_000, 9);
    let s = server(data.clone(), "\"v1\"", vec![Fault::TrickleAfter(400_000)]);
    let url = serve(s.clone()).await;
    let part = part_path("trickle");
    let config = ResumeConfig {
        min_bytes_per_second: 100_000,
        ..config(300, 5)
    };
    // At 2 KB/s the trickle alone would take five minutes.
    let total = tokio::time::timeout(
        Duration::from_secs(10),
        download_resumable(&reqwest::Client::new(), &url, &part, &config, |_, _| {}),
    )
    .await
    .expect("the trickling connection was not left")
    .unwrap();
    assert_eq!(total, 1_000_000);
    assert_eq!(std::fs::read(&part).unwrap(), data);
    let requests = s.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 2, "{requests:?}");
    // The new connection continued after what had trickled in, instead of starting over.
    assert!(
        requests[1].is_some_and(|start| start >= 400_000),
        "{requests:?}"
    );
}

#[tokio::test]
async fn continues_a_part_left_by_an_earlier_run() {
    let data = test_data(1_000_000, 3);
    let part = part_path("earlier-run");
    std::fs::write(&part, &data[..400_000]).unwrap();
    std::fs::write(part.with_extension("part.validator"), "\"v1\"").unwrap();
    let s = server(data.clone(), "\"v1\"", vec![]);
    let url = serve(s.clone()).await;
    download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(2000, 5),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), data);
    assert_eq!(s.lock().unwrap().requests, vec![Some(400_000)]);
}

#[tokio::test]
async fn starts_over_when_the_file_changed() {
    let new = test_data(1_000_000, 4);
    let part = part_path("changed");
    std::fs::write(&part, test_data(600_000, 99)).unwrap(); // part of an older upload
    std::fs::write(part.with_extension("part.validator"), "\"old\"").unwrap();
    let s = server(new.clone(), "\"new\"", vec![]);
    let url = serve(s.clone()).await;
    download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(2000, 5),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), new);
    // If-Range did not match, so the server sent the whole new file.
    assert_eq!(s.lock().unwrap().body_bytes_sent, 1_000_000);
}

#[tokio::test]
async fn a_part_without_validator_is_not_trusted() {
    let data = test_data(500_000, 5);
    let part = part_path("no-validator");
    std::fs::write(&part, test_data(200_000, 77)).unwrap();
    let s = server(data.clone(), "\"v1\"", vec![]);
    let url = serve(s.clone()).await;
    download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(2000, 5),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), data);
    assert_eq!(s.lock().unwrap().requests, vec![None]);
}

#[tokio::test]
async fn a_complete_part_finishes_at_once() {
    let data = test_data(300_000, 6);
    let part = part_path("complete");
    std::fs::write(&part, &data).unwrap();
    std::fs::write(part.with_extension("part.validator"), "\"v1\"").unwrap();
    let s = server(data.clone(), "\"v1\"", vec![]);
    let url = serve(s.clone()).await;
    let total = download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(2000, 5),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(total, 300_000);
    assert_eq!(std::fs::read(&part).unwrap(), data);
    assert_eq!(s.lock().unwrap().body_bytes_sent, 0);
}

#[tokio::test]
async fn gives_up_when_nothing_arrives() {
    let s = server(
        test_data(100_000, 7),
        "\"v1\"",
        vec![Fault::StallAfter(0); 10],
    );
    let url = serve(s.clone()).await;
    let part = part_path("dead");
    let err = download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(200, 3),
        |_, _| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ResumableError::NoProgress(3)), "{err}");
}

#[tokio::test]
async fn gives_up_with_the_last_server_error() {
    let s = server(test_data(10_000, 12), "\"v1\"", vec![Fault::Status(503); 5]);
    let url = serve(s.clone()).await;
    let part = part_path("server-error");
    let err = download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(2000, 3),
        |_, _| {},
    )
    .await
    .unwrap_err();
    // The launcher reports the server's answer (E2004), not a stalled download (E2006).
    assert!(
        matches!(err, ResumableError::Status(code) if code.as_u16() == 503),
        "{err}"
    );
    assert_eq!(s.lock().unwrap().requests.len(), 3);
}

#[tokio::test]
async fn a_missing_file_size_is_not_retried() {
    let s = server(test_data(10_000, 11), "\"v1\"", vec![Fault::NoLength; 3]);
    let url = serve(s.clone()).await;
    let part = part_path("no-length");
    let err = download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(2000, 3),
        |_, _| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ResumableError::NoContentLength), "{err}");
    assert_eq!(s.lock().unwrap().requests.len(), 1);
}

#[tokio::test]
async fn reports_a_part_that_cannot_be_created() {
    let s = server(test_data(10_000, 10), "\"v1\"", vec![]);
    let url = serve(s.clone()).await;
    // A directory that does not exist: neither the validator nor the part can be created.
    let dir = std::env::temp_dir()
        .join("resume-test")
        .join("missing-directory");
    let _ = std::fs::remove_dir_all(&dir);
    let part = dir.join("x.zip.part");
    let err = download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(2000, 3),
        |_, _| {},
    )
    .await
    .unwrap_err();
    // The launcher reports it as E1007_FILE_CREATE_FAILED, as before this change.
    assert!(matches!(err, ResumableError::CreateFile { .. }), "{err}");
    assert_eq!(s.lock().unwrap().requests.len(), 1);
}

#[tokio::test]
async fn reports_progress_to_the_end() {
    let data = test_data(1_000_000, 8);
    let s = server(data.clone(), "\"v1\"", vec![Fault::DropAfter(250_000)]);
    let url = serve(s.clone()).await;
    let part = part_path("progress");
    let mut seen = Vec::new();
    download_resumable(
        &reqwest::Client::new(),
        &url,
        &part,
        &config(2000, 5),
        |done, total| seen.push((done, total)),
    )
    .await
    .unwrap();
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "progress never goes back"
    );
    assert_eq!(seen.last(), Some(&(1_000_000, 1_000_000)));
}
