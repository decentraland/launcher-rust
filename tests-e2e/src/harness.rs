use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use dcl_launcher_ipc::protocol::{Command as IpcCommand, Response, ResponseData};
use dcl_launcher_shared::environment::Args;
use dcl_launcher_shared::types::Status;
use dcl_launcher_ipc::transport::IpcClient;
use dcl_launcher_ipc::{PROTOCOL_VERSION, protocol::ServiceState};

use crate::mock_cdn::MockCdn;

static ENV_COUNTER: AtomicU32 = AtomicU32::new(0);

pub const DEFAULT_VERSION: &str = "v9.9.1";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_POLL: Duration = Duration::from_millis(100);

/// One hermetic launcher-service universe.
///
/// Own base dir, own IPC endpoint, own mock CDN — nothing touches the real
/// user profile or the default endpoint. Dropping it kills spawned services
/// and (unless the test failed) removes the base dir.
pub struct TestEnv {
    pub base: PathBuf,
    pub endpoint: String,
    pub cdn: MockCdn,
    children: Mutex<Vec<Child>>,
}

impl TestEnv {
    pub fn new(stub_exe: &str) -> Result<Self> {
        Self::with_version(stub_exe, DEFAULT_VERSION)
    }

    pub fn with_version(stub_exe: &str, version: &str) -> Result<Self> {
        let seq = ENV_COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = temp_root().join(format!("dcle2e-{}-{}", std::process::id(), seq));
        fs::create_dir_all(&base).context("Cannot create the test base dir")?;

        let zip = build_explorer_zip(Path::new(stub_exe))?;
        let cdn = MockCdn::start(version, zip)?;
        let endpoint = format!("e2e{}x{}", std::process::id(), seq);

        Ok(Self {
            base,
            endpoint,
            cdn,
            children: Mutex::new(Vec::new()),
        })
    }

    pub fn app_dir(&self) -> PathBuf {
        self.base.join("DecentralandLauncherLight")
    }

    pub fn latest_dir(&self) -> PathBuf {
        self.app_dir().join("latest")
    }

    pub fn spawn_service(&self) -> Result<()> {
        self.spawn_service_args(&[])
    }

    pub fn spawn_service_args(&self, extra: &[&str]) -> Result<()> {
        let exe = service_exe()?;
        let seq = {
            let guard = self.children.lock().unwrap_or_else(PoisonError::into_inner);
            guard.len()
        };
        let stdout = File::create(self.base.join(format!("service-{seq}-stdout.log")))?;
        let stderr = File::create(self.base.join(format!("service-{seq}-stderr.log")))?;

        // No argv is passed on purpose: the service must not read arguments
        // from its command line — flow commands carry them (see `test_args`).
        let child = Command::new(exe)
            .args(extra)
            .env("DCL_LAUNCHER_BASE_DIR", &self.base)
            .env("DCL_LAUNCHER_IPC_ENDPOINT", &self.endpoint)
            .env("DCL_LAUNCHER_BUCKET_URL", self.cdn.base_url())
            // Core writes its log relative to APPDATA (win) / HOME (mac);
            // redirect both so even logs stay inside the sandbox.
            .env("APPDATA", &self.base)
            .env("HOME", &self.base)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .context("Cannot spawn the service under test")?;

        self.children
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(child);
        Ok(())
    }

    /// Waits for the most recently spawned service child to exit.
    pub fn wait_last_child_exit(&self, timeout: Duration) -> Result<i32> {
        let deadline = std::time::Instant::now()
            .checked_add(timeout)
            .context("Deadline overflow")?;
        loop {
            {
                let mut guard = self.children.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(child) = guard.last_mut() {
                    if let Some(status) = child.try_wait()? {
                        return Ok(status.code().unwrap_or(-1));
                    }
                } else {
                    bail!("No service child was spawned");
                }
            }
            if std::time::Instant::now() >= deadline {
                bail!("The service child did not exit within {timeout:?}");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub async fn connect(&self) -> Result<IpcClient> {
        let deadline = tokio::time::Instant::now()
            .checked_add(CONNECT_TIMEOUT)
            .context("Deadline overflow")?;
        loop {
            match IpcClient::connect_to(Some(&self.endpoint)).await {
                Ok(client) => return Ok(client),
                Err(e) => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(anyhow!(
                            "Cannot connect to the service endpoint {} within {CONNECT_TIMEOUT:?}: {e}\n{}",
                            self.endpoint,
                            self.diagnostics()
                        ));
                    }
                    tokio::time::sleep(CONNECT_POLL).await;
                }
            }
        }
    }

    /// Connects and performs the hello handshake; returns the client and the
    /// service version it reported.
    pub async fn client(&self) -> Result<(IpcClient, String)> {
        let mut client = self.connect().await?;
        let response = client
            .request_silent(IpcCommand::Hello {
                protocol_version: PROTOCOL_VERSION,
                app_version: "e2e-harness".to_owned(),
            })
            .await?;
        let Some(ResponseData::Hello {
            service_version, ..
        }) = response.data
        else {
            bail!("Unexpected hello response: {response:?}");
        };
        Ok((client, service_version))
    }

    pub async fn launch_collect(
        &self,
        client: &mut IpcClient,
        deeplink: Option<String>,
    ) -> Result<(Response, Vec<Status>)> {
        let mut events = Vec::new();
        let response = client
            .request(
                IpcCommand::Launch {
                    deeplink,
                    args: test_args(),
                },
                |status| {
                    events.push(status);
                },
            )
            .await?;
        Ok((response, events))
    }

    pub async fn view_state(&self, client: &mut IpcClient) -> Result<ServiceState> {
        let response = client.request_silent(IpcCommand::ViewCurrentState).await?;
        match response.data {
            Some(ResponseData::CurrentState { state }) => Ok(state),
            other => bail!("Unexpected viewCurrentState response: {other:?}"),
        }
    }

    // ---- stub explorer helpers -------------------------------------------

    pub fn stub_launches(&self) -> Vec<serde_json::Value> {
        read_jsonl(&self.latest_dir().join("stub-launches.jsonl"))
    }

    pub async fn wait_stub_launches(
        &self,
        expected: usize,
        timeout: Duration,
    ) -> Result<Vec<serde_json::Value>> {
        let deadline = tokio::time::Instant::now()
            .checked_add(timeout)
            .context("Deadline overflow")?;
        loop {
            let launches = self.stub_launches();
            if launches.len() >= expected {
                return Ok(launches);
            }
            if tokio::time::Instant::now() >= deadline {
                bail!(
                    "Expected {expected} stub launches, found {} within {timeout:?}\n{}",
                    launches.len(),
                    self.diagnostics()
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub fn request_stub_exit(&self, code: i32) {
        let _ = fs::write(
            self.latest_dir().join("stub-exit-all.txt"),
            code.to_string(),
        );
    }

    pub fn clear_stub_exit(&self) {
        let _ = fs::remove_file(self.latest_dir().join("stub-exit-all.txt"));
    }

    pub fn set_ignore_bridge(&self, ignore: bool) {
        let flag = self.latest_dir().join("stub-ignore-bridge.txt");
        if ignore {
            let _ = fs::write(flag, "1");
        } else {
            let _ = fs::remove_file(flag);
        }
    }

    pub fn bridge_log(&self) -> Vec<serde_json::Value> {
        read_jsonl(&self.latest_dir().join("stub-bridge-log.jsonl"))
    }

    // ---- inspection -------------------------------------------------------

    pub fn pidfile(&self) -> Option<serde_json::Value> {
        let raw = fs::read_to_string(self.app_dir().join("current-service-pid.txt")).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn service_log(&self) -> String {
        #[cfg(windows)]
        let path = self
            .base
            .join("DecentralandLauncherLight")
            .join("output.log");
        #[cfg(unix)]
        let path = self
            .base
            .join("Library/Logs/DecentralandLauncherLight/output.log");
        fs::read_to_string(path).unwrap_or_default()
    }

    pub fn diagnostics(&self) -> String {
        let mut out = format!("--- test env base: {}\n", self.base.display());
        out.push_str("--- mock CDN requests:\n");
        for entry in self.cdn.snapshot(|s| s.request_log.clone()) {
            out.push_str(&entry);
            out.push('\n');
        }
        let log = self.service_log();
        let tail: Vec<&str> = log.lines().rev().take(30).collect();
        out.push_str("--- service log tail:\n");
        for line in tail.iter().rev() {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let mut guard = self.children.lock().unwrap_or_else(PoisonError::into_inner);
        for child in guard.iter_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        drop(guard);

        // Killed services never reach their graceful endpoint cleanup.
        #[cfg(unix)]
        let _ = fs::remove_file(dcl_launcher_ipc::transport::socket_path_for(Some(
            &self.endpoint,
        )));

        if std::thread::panicking() {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(
                stderr,
                "[e2e] test failed; keeping the sandbox for inspection:\n{}",
                self.diagnostics()
            );
        } else {
            let _ = fs::remove_dir_all(&self.base);
        }
    }
}

/// The arguments every test flow command carries: the service builds its
/// analytics client from the first command's args, so analytics must be
/// disabled there to keep the sandbox hermetic.
pub fn test_args() -> Args {
    Args {
        skip_analytics: true,
        ..Args::default()
    }
}

fn temp_root() -> PathBuf {
    // /tmp keeps sandbox paths short (std::env::temp_dir() is the long
    // /var/folders/... on macOS). Sockets live in /tmp too — see
    // `dcl_launcher_ipc::transport::socket_path_for`.
    #[cfg(unix)]
    {
        PathBuf::from("/tmp")
    }
    #[cfg(windows)]
    {
        std::env::temp_dir()
    }
}

fn read_jsonl(path: &Path) -> Vec<serde_json::Value> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Locates the debug service binary; built by `scripts/run-e2e.rs`.
pub fn service_exe() -> Result<PathBuf> {
    let name = if cfg!(windows) {
        "dcl_launcher_service.exe"
    } else {
        "dcl_launcher_service"
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("tests-e2e has no parent dir")?
        .join("src-service")
        .join("target")
        .join("debug")
        .join(name);
    if !path.exists() {
        bail!(
            "Service binary not found at {}.\nBuild it first: cargo build --manifest-path src-service/Cargo.toml (or run `npm run e2e`)",
            path.display()
        );
    }
    Ok(path)
}

/// Builds the fake Explorer artifact the mock CDN serves: the platform zip
/// layout the installer expects, containing the stub binary.
pub fn build_explorer_zip(stub_exe: &Path) -> Result<Vec<u8>> {
    use zip::write::SimpleFileOptions;

    let stub_bytes = fs::read(stub_exe)
        .with_context(|| format!("Cannot read the stub binary at {}", stub_exe.display()))?;

    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);

    #[cfg(windows)]
    {
        writer.start_file("Decentraland.exe", SimpleFileOptions::default())?;
        writer.write_all(&stub_bytes)?;
    }

    #[cfg(unix)]
    {
        const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>Explorer</string>
    <key>CFBundleIdentifier</key><string>org.decentraland.e2e-stub</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleName</key><string>Decentraland</string>
</dict>
</plist>
"#;
        // Real release zips carry explicit directory entries; keep the
        // fixture faithful to them.
        for dir in [
            "build",
            "build/Decentraland.app",
            "build/Decentraland.app/Contents",
            "build/Decentraland.app/Contents/MacOS",
        ] {
            writer.add_directory(dir, SimpleFileOptions::default())?;
        }
        writer.start_file(
            "build/Decentraland.app/Contents/Info.plist",
            SimpleFileOptions::default(),
        )?;
        writer.write_all(PLIST.as_bytes())?;
        writer.start_file(
            "build/Decentraland.app/Contents/MacOS/Explorer",
            SimpleFileOptions::default().unix_permissions(0o755),
        )?;
        writer.write_all(&stub_bytes)?;
    }

    let cursor = writer.finish()?;
    Ok(cursor.into_inner())
}
