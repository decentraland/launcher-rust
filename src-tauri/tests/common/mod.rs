#![allow(dead_code)]

//! Shared sandbox for the Layer 2 integration tests. Each test FILE is its
//! own process, so process-global env (base dir, endpoint) is safe to set
//! once at the start of the single test in that file.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};

pub struct Sandbox {
    pub base: PathBuf,
    pub endpoint: String,
}

pub fn init(tag: &str) -> Result<Sandbox> {
    // /tmp keeps sandbox paths short (std::env::temp_dir() is the long
    // /var/folders/... on macOS). Sockets live in /tmp too — see
    // `dcl_launcher_ipc::transport::socket_path_for`.
    #[cfg(unix)]
    let temp_root = PathBuf::from("/tmp");
    #[cfg(windows)]
    let temp_root = std::env::temp_dir();

    let base = temp_root.join(format!("dcll2-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&base).context("Cannot create the sandbox dir")?;
    let endpoint = format!("l2{}{tag}", std::process::id());

    // Safe: called once, before any threads that read these.
    std::env::set_var("DCL_LAUNCHER_BASE_DIR", &base);
    std::env::set_var("DCL_LAUNCHER_IPC_ENDPOINT", &endpoint);
    std::env::set_var(
        "DCL_LAUNCHER_BUCKET_URL",
        "http://127.0.0.1:9/l2-placeholder",
    );
    std::env::set_var("APPDATA", &base);
    std::env::set_var("HOME", &base);

    stage_service_binary()?;

    Ok(Sandbox { base, endpoint })
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // Stop whatever service the test left running, then clean up.
        if let Some(entry) = dcl_launcher_ipc::pidfile::read() {
            if dcl_launcher_ipc::pidfile::is_alive(&entry) {
                dcl_launcher_ipc::pidfile::kill(&entry);
            }
        }
        // Killed services never reach their graceful endpoint cleanup.
        #[cfg(unix)]
        let _ = std::fs::remove_file(dcl_launcher_ipc::transport::socket_path_for(Some(
            &self.endpoint,
        )));
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.base);
        } else {
            eprintln!("[l2] test failed; sandbox kept at {}", self.base.display());
        }
    }
}

/// `ensure_service` spawns the sibling `dcl_launcher_service` next to the
/// current executable — for a test binary that is `target/debug/deps/`, so
/// stage the debug service binary there.
fn stage_service_binary() -> Result<PathBuf> {
    let name = if cfg!(windows) {
        "dcl_launcher_service.exe"
    } else {
        "dcl_launcher_service"
    };
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("src-tauri has no parent dir")?
        .join("src-service")
        .join("target")
        .join("debug")
        .join(name);
    if !source.exists() {
        bail!(
            "Service binary not found at {}. Build it first: cargo build --manifest-path src-service/Cargo.toml",
            source.display()
        );
    }

    let target_dir = std::env::current_exe()
        .context("Cannot resolve the test executable")?
        .parent()
        .context("Test executable has no parent dir")?
        .to_path_buf();
    let target = target_dir.join(name);

    // Copy only when missing or stale; a running service from a parallel test
    // file may hold a lock on the existing copy.
    let stale = match (source.metadata(), target.metadata()) {
        (Ok(s), Ok(t)) => s.modified().ok() > t.modified().ok(),
        (_, Err(_)) => true,
        _ => false,
    };
    if stale {
        std::fs::copy(&source, &target).context("Cannot stage the service binary")?;
    }
    Ok(target)
}

pub fn service_binary_source_dir() -> Result<PathBuf> {
    Ok(std::env::current_exe()?
        .parent()
        .context("no parent")?
        .to_path_buf())
}

pub async fn wait_pidfile_alive(
    timeout: Duration,
) -> Result<dcl_launcher_ipc::pidfile::ServicePid> {
    let deadline = tokio::time::Instant::now()
        .checked_add(timeout)
        .context("Deadline overflow")?;
    loop {
        if let Some(entry) = dcl_launcher_ipc::pidfile::read() {
            if dcl_launcher_ipc::pidfile::is_alive(&entry) {
                return Ok(entry);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            bail!("No live service pidfile appeared within {timeout:?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
