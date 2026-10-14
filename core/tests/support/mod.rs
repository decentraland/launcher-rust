//! Harness shared by the integration tests: an isolated app directory, a bucket served from
//! localhost and a channel that records every `Status` the flow emits.
//!
//! The app directory override is process-global, so [`TestEnv::new`] serializes the tests of
//! one binary through a mutex. Every test owns its own temp dirs and bucket port.

#![allow(dead_code)]

use dcl_launcher_core::analytics::Analytics;
use dcl_launcher_core::channel::EventChannel;
use dcl_launcher_core::installs::{self, InstallsHub};
use dcl_launcher_core::instances::RunningInstances;
use dcl_launcher_core::launch_flow::{LaunchFlow, LaunchFlowState};
use dcl_launcher_core::types::Status;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

pub const RELEASE_PREFIX: &str = "@dcl/unity-explorer/releases";

#[cfg(target_os = "windows")]
pub const PLATFORM: &str = "windows64";
#[cfg(target_os = "macos")]
pub const PLATFORM: &str = "macos";

/// Path of the Explorer executable inside an installed build folder.
#[cfg(target_os = "windows")]
pub const EXPLORER_BIN: &str = "Decentraland.exe";
#[cfg(target_os = "macos")]
pub const EXPLORER_BIN: &str = "Decentraland.app/Contents/MacOS/Explorer";

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// One test's world: holds the serialization lock, the app directory and the bucket server.
pub struct TestEnv {
    _lock: MutexGuard<'static, ()>,
    pub data_dir: PathBuf,
    pub bucket: Bucket,
}

impl TestEnv {
    pub fn new() -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        // `RUST_LOG=info cargo test -- --nocapture` shows the flow's own log lines.
        let _ = pretty_env_logger::try_init();
        let root = std::env::temp_dir().join(format!("dcl-launcher-it-{}", uuid::Uuid::new_v4()));
        let data_dir = root.join("data");
        let bucket_dir = root.join("bucket");
        fs::create_dir_all(&data_dir).expect("create data dir");
        fs::create_dir_all(&bucket_dir).expect("create bucket dir");

        set_data_dir(Some(&data_dir));

        let bucket = Bucket::serve(bucket_dir);
        let config = serde_json::json!({
            "cmd-arguments": format!("--use-bucket-url {} --skip-analytics", bucket.base_url),
        });
        fs::write(
            data_dir.join("config.json"),
            serde_json::to_string_pretty(&config).expect("serialize config"),
        )
        .expect("write config.json");

        Self {
            _lock: lock,
            data_dir,
            bucket,
        }
    }

    pub fn latest_dir(&self) -> PathBuf {
        self.data_dir.join("latest")
    }

    pub fn version_dir(&self, version: &str) -> PathBuf {
        self.data_dir.join(version)
    }

    /// `version` recorded in `version.json`, if any.
    pub fn installed_version(&self) -> Option<String> {
        let text = fs::read_to_string(self.data_dir.join("version.json")).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        value.get("version")?.as_str().map(ToOwned::to_owned)
    }

    /// A flow and its state built the same way `app.rs` builds them, minus analytics.
    pub fn new_flow(&self) -> (LaunchFlow, Arc<tokio::sync::Mutex<LaunchFlowState>>) {
        let analytics = Arc::new(tokio::sync::Mutex::new(Analytics::new(None)));
        let running_instances = Arc::new(tokio::sync::Mutex::new(RunningInstances::default()));
        let installs_hub = Arc::new(tokio::sync::Mutex::new(InstallsHub::new(
            analytics.clone(),
            running_instances.clone(),
        )));
        let flow = LaunchFlow::new(installs_hub, analytics, running_instances);
        let state = Arc::new(tokio::sync::Mutex::new(LaunchFlowState::default()));
        (flow, state)
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        set_data_dir(None);
        if let Some(root) = self.data_dir.parent() {
            let _ = fs::remove_dir_all(root);
        }
    }
}

/// The tests run serialized under `ENV_LOCK`, so no other thread reads the environment while it
/// changes; that is the contract `set_var` asks for since edition 2024.
#[allow(unsafe_code)]
fn set_data_dir(dir: Option<&Path>) {
    unsafe {
        match dir {
            Some(dir) => std::env::set_var(installs::DATA_DIR_ENV, dir),
            None => std::env::remove_var(installs::DATA_DIR_ENV),
        }
    }
}

/// Fault injection for the bucket server, keyed by the request path relative to the bucket
/// root (for example `@dcl/unity-explorer/releases/v0.2.0/delta/windows64/manifest.json`).
#[derive(Default)]
pub struct Faults {
    /// Answer 404 for these paths even if the file exists.
    pub not_found: Vec<String>,
    /// Serve these files with their bytes corrupted.
    pub corrupt: Vec<String>,
    /// Announce the full length but close the connection after this many bytes.
    pub truncate: Vec<(String, usize)>,
}

/// A folder of release artifacts served over HTTP from `127.0.0.1`.
pub struct Bucket {
    pub root: PathBuf,
    pub base_url: String,
    pub faults: Arc<Mutex<Faults>>,
    server: Arc<tiny_http::Server>,
    thread: Option<JoinHandle<()>>,
}

impl Bucket {
    fn serve(root: PathBuf) -> Self {
        let server =
            Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("bind bucket server"));
        let base_url = format!("http://{}", server.server_addr());
        let faults = Arc::new(Mutex::new(Faults::default()));
        let thread = {
            let server = server.clone();
            let root = root.clone();
            let faults = faults.clone();
            std::thread::spawn(move || serve_requests(&server, &root, &faults))
        };
        Self {
            root,
            base_url,
            faults,
            server,
            thread: Some(thread),
        }
    }

    pub fn releases_dir(&self) -> PathBuf {
        self.root.join(RELEASE_PREFIX)
    }

    /// Writes a file under the bucket root, creating parents.
    pub fn put(&self, relative: &str, bytes: &[u8]) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create bucket parents");
        }
        fs::write(path, bytes).expect("write bucket file");
    }

    pub fn publish_latest(&self, version: &str) {
        let latest = serde_json::json!({ "version": version });
        self.put(
            &format!("{RELEASE_PREFIX}/latest.json"),
            latest.to_string().as_bytes(),
        );
    }

    pub fn publish_zip(&self, version: &str, zip: &[u8]) {
        self.put(
            &format!("{RELEASE_PREFIX}/{version}/Decentraland_{PLATFORM}.zip"),
            zip,
        );
    }

    pub fn with_faults(&self, edit: impl FnOnce(&mut Faults)) {
        let mut faults = self.faults.lock().unwrap_or_else(PoisonError::into_inner);
        edit(&mut faults);
    }
}

impl Drop for Bucket {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve_requests(server: &tiny_http::Server, root: &Path, faults: &Mutex<Faults>) {
    while let Ok(request) = server.recv() {
        let relative = request
            .url()
            .split('?')
            .next()
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_owned();
        let response = build_response(root, &relative, faults);
        let _ = request.respond(response);
    }
}

type BoxedResponse = tiny_http::Response<Box<dyn std::io::Read + Send>>;

fn build_response(root: &Path, relative: &str, faults: &Mutex<Faults>) -> BoxedResponse {
    let faults = faults.lock().unwrap_or_else(PoisonError::into_inner);
    let not_found = || {
        tiny_http::Response::from_string("not found")
            .with_status_code(404)
            .boxed()
    };
    if faults.not_found.iter().any(|p| p == relative) {
        return not_found();
    }
    let Ok(mut bytes) = fs::read(root.join(relative)) else {
        return not_found();
    };
    let full_len = bytes.len();
    if faults.corrupt.iter().any(|p| p == relative) {
        for byte in bytes.iter_mut().take(64) {
            *byte ^= 0xFF;
        }
    }
    if let Some((_, keep)) = faults.truncate.iter().find(|(p, _)| p == relative) {
        bytes.truncate(*keep);
    }
    tiny_http::Response::new(
        tiny_http::StatusCode(200),
        Vec::new(),
        Box::new(Cursor::new(bytes)) as Box<dyn std::io::Read + Send>,
        Some(full_len),
        None,
    )
}

/// Records every status the flow sends.
#[derive(Clone, Default)]
pub struct CollectingChannel {
    statuses: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl CollectingChannel {
    pub fn statuses(&self) -> Vec<serde_json::Value> {
        self.statuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The `step` event names in order, with consecutive duplicates collapsed
    /// (`fetching`, `downloading`, `installing`, ...).
    pub fn step_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for status in self.statuses() {
            let Some(name) = status
                .pointer("/data/data/step/event")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            if names.last().map(String::as_str) != Some(name) {
                names.push(name.to_owned());
            }
        }
        names
    }

    /// `buildType` of the first `downloading` status (`new` or `update`).
    pub fn download_build_type(&self) -> Option<String> {
        self.statuses().iter().find_map(|s| {
            s.pointer("/data/data/step/data/buildType/event")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
    }
}

impl EventChannel for CollectingChannel {
    fn send(&self, status: Status) -> anyhow::Result<()> {
        let value = serde_json::to_value(&status)?;
        self.statuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(value);
        Ok(())
    }
}

/// One file of a synthetic Explorer build.
pub struct BuildFile {
    pub path: String,
    pub content: Vec<u8>,
    pub mode: u32,
}

/// The smallest build the launcher accepts as installed: the executable plus one data file,
/// both carrying `version` so a test can tell which build landed in `latest/`.
pub fn build_files(version: &str) -> Vec<BuildFile> {
    #[cfg_attr(target_os = "windows", allow(unused_mut))]
    let mut files = vec![
        BuildFile {
            path: EXPLORER_BIN.to_owned(),
            content: format!("explorer binary {version}").into_bytes(),
            mode: 0o755,
        },
        BuildFile {
            path: data_file_path(),
            content: format!("data {version}").into_bytes(),
            mode: 0o644,
        },
    ];
    #[cfg(target_os = "macos")]
    files.push(BuildFile {
        path: "Decentraland.app/Contents/Info.plist".to_owned(),
        content: INFO_PLIST.as_bytes().to_vec(),
        mode: 0o644,
    });
    files
}

#[cfg(target_os = "windows")]
pub fn data_file_path() -> String {
    "Decentraland_Data/level0".to_owned()
}
#[cfg(target_os = "macos")]
pub fn data_file_path() -> String {
    "Decentraland.app/Contents/Resources/Data/level0".to_owned()
}

#[cfg(target_os = "macos")]
const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>Explorer</string>
	<key>CFBundleIdentifier</key>
	<string>org.decentraland.explorer</string>
</dict>
</plist>
"#;

/// Packs `files` the way the producer ships a release for this platform: a plain zip on
/// Windows, a zip wrapping `build.tar` (with every path under `build/`) on macOS.
pub fn build_zip(files: &[BuildFile]) -> Vec<u8> {
    #[cfg(target_os = "windows")]
    {
        plain_zip(files)
    }
    #[cfg(target_os = "macos")]
    {
        tar_zip(files)
    }
}

/// A zip for the build of `version` as `build_files` lays it out.
pub fn release_zip(version: &str) -> Vec<u8> {
    build_zip(&build_files(version))
}

#[cfg(target_os = "windows")]
fn plain_zip(files: &[BuildFile]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default();
        for file in files {
            zip.start_file(file.path.as_str(), options)
                .expect("zip entry");
            zip.write_all(&file.content).expect("zip content");
        }
        zip.finish().expect("zip finish");
    }
    cursor.into_inner()
}

#[cfg(target_os = "macos")]
fn tar_zip(files: &[BuildFile]) -> Vec<u8> {
    let mut tar_data = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut tar_data);
        for file in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(file.content.len() as u64);
            header.set_mode(file.mode);
            header.set_cksum();
            tar.append_data(
                &mut header,
                format!("build/{}", file.path),
                file.content.as_slice(),
            )
            .expect("tar entry");
        }
        tar.finish().expect("tar finish");
    }
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        zip.start_file("build.tar", zip::write::SimpleFileOptions::default())
            .expect("zip entry");
        zip.write_all(&tar_data).expect("zip content");
        zip.finish().expect("zip finish");
    }
    cursor.into_inner()
}
