//! Harness shared by the integration tests: an isolated app directory, a bucket served from
//! localhost, and the launch FSM run the way `app.rs` runs it, minus analytics and the UI.
//!
//! The app directory override is process-global, so [`TestEnv::new`] serializes the tests of
//! one binary through a mutex. Every test owns its own temp dirs and bucket port.

#![allow(dead_code)]

use dcl_launcher_core::analytics::Analytics;
use dcl_launcher_core::delta::sha256_file;
use dcl_launcher_core::environment::AppEnvironment;
use dcl_launcher_core::errors::DCLError;
use dcl_launcher_core::fsm::{
    self, Config, Executor, ExplorerLaunch, LaunchIntentionOutcome, Msg, State,
};
use dcl_launcher_core::infra::fsm::FsmRuntime;
use dcl_launcher_core::installs::{self, InstallsHub};
use dcl_launcher_core::instances::RunningInstances;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

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

/// `hdiffz`, needed to make patches for the delta tests. Unset: those tests are skipped.
pub const HDIFFZ_ENV: &str = "HDIFFZ";

const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);

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
        // `RUST_LOG=info cargo test -- --nocapture` shows the FSM's own log lines.
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

    /// Runs one launch intention through the FSM until it reports an outcome. The synthetic
    /// Explorer binary cannot start, so a fully prepared install ends with a launch failure.
    pub async fn run_launch(&self) -> LaunchRun {
        let analytics = Arc::new(tokio::sync::Mutex::new(Analytics::new(None)));
        let running_instances = Arc::new(tokio::sync::Mutex::new(RunningInstances::default()));
        let installs_hub = Arc::new(tokio::sync::Mutex::new(InstallsHub::new(
            analytics.clone(),
            running_instances.clone(),
        )));
        let config = Config {
            use_delta_mode: true,
            args: AppEnvironment::cmd_args(),
        };
        let executor = Executor::new(analytics, running_instances, installs_hub);
        let mut runtime = FsmRuntime::new(
            "test-launch",
            State::new(config),
            fsm::update,
            executor,
            tokio::runtime::Handle::current(),
        );
        runtime.start();
        runtime
            .inbox()
            .send(Msg::LaunchIntentionReceived(None))
            .expect("send the intention");

        let mut watch = runtime.watch();
        let mut statuses = Vec::new();
        let deadline = tokio::time::Instant::now() + LAUNCH_TIMEOUT;
        loop {
            let state = watch.borrow_and_update().clone();
            if let Some(status) = fsm::status(&state) {
                statuses.push(serde_json::to_value(&status).expect("serialize status"));
            }
            let outcome = fsm::launch_intention_outcome(&state);
            if outcome.is_some() {
                return LaunchRun {
                    state,
                    statuses,
                    outcome,
                };
            }
            let changed = tokio::time::timeout_at(deadline, watch.changed()).await;
            assert!(
                changed.is_ok(),
                "the launch did not settle within {LAUNCH_TIMEOUT:?}"
            );
        }
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

/// What one launch intention ended with.
pub struct LaunchRun {
    pub state: State,
    pub statuses: Vec<serde_json::Value>,
    pub outcome: Option<LaunchIntentionOutcome>,
}

impl LaunchRun {
    /// The `step` event names in order, with consecutive duplicates collapsed.
    pub fn step_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for status in &self.statuses {
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
        self.statuses.iter().find_map(|s| {
            s.pointer("/data/data/step/data/buildType/event")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
    }

    /// The error the run ended with, if it failed.
    pub fn error(&self) -> Option<&DCLError> {
        match &self.outcome {
            Some(LaunchIntentionOutcome::Failed(error)) => Some(error),
            _ => None,
        }
    }

    /// The install was complete and the only failure is starting the synthetic binary.
    pub fn reached_the_launch(&self) -> bool {
        matches!(self.state.launch, ExplorerLaunch::Failed { .. })
            && self
                .error()
                .is_some_and(|e| e.code().starts_with("E3010") || e.code().starts_with("E3012"))
    }

    /// The message of the first `error` status the UI received.
    pub fn error_status_message(&self) -> Option<String> {
        self.statuses.iter().find_map(|s| {
            (s.pointer("/data/event")?.as_str()? == "error").then(|| {
                s.pointer("/data/data/message")?
                    .as_str()
                    .map(ToOwned::to_owned)
            })?
        })
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
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("bind bucket server"));
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

    pub fn delta_prefix(version: &str) -> String {
        format!("{RELEASE_PREFIX}/{version}/delta/{PLATFORM}")
    }

    pub fn manifest_key(version: &str) -> String {
        format!("{}/manifest.json", Self::delta_prefix(version))
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

/// One file of a synthetic Explorer build.
#[derive(Clone)]
pub struct BuildFile {
    pub path: String,
    pub content: Vec<u8>,
    pub mode: u32,
}

impl BuildFile {
    pub fn new(path: &str, content: &str, mode: u32) -> Self {
        Self {
            path: path.to_owned(),
            content: content.as_bytes().to_vec(),
            mode,
        }
    }
}

/// The smallest build the launcher accepts as installed: the executable plus one data file,
/// both carrying `version` so a test can tell which build landed in `latest/`.
pub fn build_files(version: &str) -> Vec<BuildFile> {
    #[cfg_attr(target_os = "windows", allow(unused_mut))]
    let mut files = vec![
        BuildFile::new(EXPLORER_BIN, &format!("explorer binary {version}"), 0o755),
        BuildFile::new(&data_file_path(), &format!("data {version}"), 0o644),
    ];
    #[cfg(target_os = "macos")]
    files.push(BuildFile::new(
        "Decentraland.app/Contents/Info.plist",
        INFO_PLIST,
        0o644,
    ));
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

// ---- Delta sets, produced with the real `hdiffz` the way the producer does ----------------

/// `hdiffz` from `$HDIFFZ`; `None` means the delta tests are skipped on this machine.
pub fn hdiffz() -> Option<PathBuf> {
    let path = std::env::var_os(HDIFFZ_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)?;
    if !path.is_file() {
        eprintln!("skipping: {HDIFFZ_ENV}={} is not a file", path.display());
        return None;
    }
    if std::env::var_os(dcl_launcher_core::delta::patcher::PATCHER_ENV).is_none() {
        eprintln!(
            "skipping: {} is not set",
            dcl_launcher_core::delta::patcher::PATCHER_ENV
        );
        return None;
    }
    Some(path)
}

/// A manifest plus its patch files for `from` -> `to`, laid out like the bucket folder
/// `<releases>/<to>/delta/<platform>/`.
pub struct DeltaSet {
    pub manifest: serde_json::Value,
    /// Patch key relative to the delta folder, and its bytes.
    pub patches: Vec<(String, Vec<u8>)>,
}

/// Publishes a delta set from `base_files` (installed as `from`) to `target_files` (`to`).
/// Files present in both with different content become patches, new files become whole
/// patches, and files absent from the target are deleted.
pub fn make_delta_set(
    hdiffz: &Path,
    from: &str,
    base_files: &[BuildFile],
    to: &str,
    target_files: &[BuildFile],
) -> DeltaSet {
    let work = std::env::temp_dir().join(format!("dcl-launcher-delta-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&work).expect("create delta work dir");
    let empty = work.join("empty");
    fs::write(&empty, b"").expect("write empty file");
    let build_id = "deadbeef-e1";

    let mut files = Vec::new();
    let mut patches = Vec::new();
    let mut patch_entries = Vec::new();
    let mut whole_entries = Vec::new();
    for target in target_files {
        let target_path = work.join("target").join(&target.path);
        write_file(&target_path, &target.content);
        let sha256 = sha256_file(&target_path).expect("hash target").to_string();
        files.push(serde_json::json!({
            "path": target.path, "size": target.content.len(), "sha256": sha256,
            "mode": format!("{:o}", target.mode),
        }));
        let base = base_files.iter().find(|b| b.path == target.path);
        let key = format!("from/{from}/{build_id}/{}.hdiff", target.path);
        match base {
            Some(base) if base.content == target.content => {}
            Some(base) => {
                let base_path = work.join("base").join(&base.path);
                write_file(&base_path, &base.content);
                let patch = encode(hdiffz, &base_path, &target_path, &work.join("patch"));
                patch_entries.push(serde_json::json!({
                    "path": target.path, "from_path": base.path,
                    "from_sha256": sha256_file(&base_path).expect("hash base").to_string(),
                    "patch": key, "patch_size": patch.len(),
                    "patch_sha256": sha256_bytes(&patch),
                }));
                patches.push((key, patch));
            }
            None => {
                let patch = encode(hdiffz, &empty, &target_path, &work.join("patch"));
                whole_entries.push(serde_json::json!({
                    "path": target.path, "patch": key, "patch_size": patch.len(),
                    "patch_sha256": sha256_bytes(&patch),
                }));
                patches.push((key, patch));
            }
        }
    }
    let deleted: Vec<&str> = base_files
        .iter()
        .filter(|b| !target_files.iter().any(|t| t.path == b.path))
        .map(|b| b.path.as_str())
        .collect();
    let _ = fs::remove_dir_all(&work);

    let manifest = serde_json::json!({
        "version": to,
        "platform": PLATFORM,
        "zip": { "path": format!("Decentraland_{PLATFORM}.zip"), "size": 1, "sha256": "a".repeat(64) },
        "files": files,
        "bases": { from: { "deleted": deleted, "patches": patch_entries, "whole": whole_entries } },
    });
    DeltaSet { manifest, patches }
}

impl DeltaSet {
    pub fn publish(&self, bucket: &Bucket, version: &str) {
        bucket.put(
            &Bucket::manifest_key(version),
            self.manifest.to_string().as_bytes(),
        );
        for (key, bytes) in &self.patches {
            bucket.put(&format!("{}/{key}", Bucket::delta_prefix(version)), bytes);
        }
    }

    pub fn patch_keys(&self, version: &str) -> Vec<String> {
        self.patches
            .iter()
            .map(|(key, _)| format!("{}/{key}", Bucket::delta_prefix(version)))
            .collect()
    }
}

fn write_file(path: &Path, content: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write file");
}

fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

fn encode(hdiffz: &Path, old: &Path, new: &Path, patch: &Path) -> Vec<u8> {
    let status = Command::new(hdiffz)
        .args(["-f", "-m-6", "-c-lzma2-9-16m"])
        .arg(old)
        .arg(new)
        .arg(patch)
        .status()
        .expect("run hdiffz");
    assert!(status.success(), "hdiffz failed: {status}");
    fs::read(patch).expect("read patch")
}
