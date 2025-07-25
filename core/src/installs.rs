use crate::analytics::Analytics;
use crate::analytics::event::Event;
use crate::environment::AppEnvironment;
use crate::errors::{StepError, StepResult};
use crate::instances::RunningInstances;
use crate::processes::CommandExtDetached;
use crate::protocols::DeepLink;
use anyhow::{Context, Error, Result, anyhow};
use semver::Version;
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::fmt;
use std::fmt::Display;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::{fs, fs::create_dir_all};
use tokio::sync::Mutex;

#[cfg(target_os = "macos")]
use std::os::unix::fs::PermissionsExt;

#[cfg(target_os = "macos")]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;

pub mod compression;
pub mod downloads;

const APP_NAME: &str = "DecentralandLauncherLight";
const EXPLORER_DOWNLOADED_FILENAME: &str = "decentraland.zip";

#[cfg(target_os = "macos")]
const EXPLORER_MAC_BIN_PATH: &str = "Decentraland.app/Contents/MacOS/Explorer";

#[cfg(target_os = "windows")]
const EXPLORER_WIN_BIN_PATH: &str = "Decentraland.exe";

pub fn log_file_path() -> Result<PathBuf> {
    let mut path = PathBuf::new();
    if let Some(dir) = dirs::home_dir() {
        path.push(dir);
    }

    #[cfg(target_os = "macos")]
    {
        path.push("Library/Logs");
    }
    #[cfg(target_os = "windows")]
    {
        let dir = std::env::var("APPDATA")?;
        path.push(dir);
    }

    path.push(APP_NAME);
    fs::create_dir_all(&path)?;

    path.push("output.log");
    Ok(path)
}

pub fn config_path() -> PathBuf {
    explorer_path().join("config.json")
}

pub fn running_instances_path() -> PathBuf {
    explorer_path().join("running-instances.json")
}

pub fn deeplink_bridge_path() -> PathBuf {
    explorer_path().join("deeplink-bridge.json")
}

// There is no point to recovery if the app failed to create working directory
#[allow(clippy::expect_used)]
fn get_app_base_path() -> PathBuf {
    dirs::data_local_dir().expect("Failed to get current directory")
}

#[allow(clippy::expect_used)]
fn explorer_path() -> PathBuf {
    let path = get_app_base_path().join(APP_NAME);
    create_dir_all(&path).expect("Cannot create app directory");
    path
}

#[allow(clippy::expect_used)]
fn explorer_downloads_path() -> PathBuf {
    let dir = explorer_path().join("downloads");
    create_dir_all(&dir).expect("Cannot create downloads directory");
    dir
}

fn explorer_version_path() -> PathBuf {
    explorer_path().join("version.json")
}

fn explorer_latest_version_path() -> Result<PathBuf> {
    let data = get_version_data()?;
    let path = data
        .get("path")
        .context("missing \"path\" property in version data")?;
    let value = path
        .as_str()
        .context("cannot get string value from path property")?;
    Ok(PathBuf::from(value))
}

fn explorer_dev_version_path() -> PathBuf {
    explorer_path().join("dev")
}

fn get_version_data() -> Result<Map<String, Value>> {
    let path = explorer_version_path();
    if path.exists() {
        let data = fs::read_to_string(path).context("Failed to read version.json")?;
        let value =
            serde_json::from_str::<serde_json::Value>(&data).context("Failed to parse JSON")?;

        return match value {
            Value::Object(obj) => Ok(obj),
            _ => return Err(anyhow!("Expected JSON object")),
        };
    }

    Err(anyhow!(format!(
        "File doesn't exists: {}",
        path.to_str().unwrap_or("no path")
    )))
}

fn get_version_data_or_empty() -> Map<String, Value> {
    get_version_data().unwrap_or_else(|e| {
        log::error!("Cannot get version data, fallback to new empty: {:#?}", e);

        Map::new()
    })
}

#[cfg(target_os = "macos")]
fn get_explorer_bin_path(version: Option<&str>) -> Result<PathBuf> {
    let base_path = match version {
        Some("dev") => explorer_dev_version_path(),
        Some(v) => explorer_path().join(v),
        None => explorer_latest_version_path()?,
    };
    Ok(base_path.join(EXPLORER_MAC_BIN_PATH))
}

#[cfg(target_os = "windows")]
fn get_explorer_bin_path(version: Option<&str>) -> Result<PathBuf> {
    let base_path = match version {
        Some("dev") => explorer_dev_version_path(),
        Some(v) => explorer_path().join(v),
        None => explorer_latest_version_path()?,
    };
    Ok(base_path.join(EXPLORER_WIN_BIN_PATH))
}

#[cfg(target_os = "macos")]
fn move_recursive(src: &PathBuf, dst: &PathBuf) -> Result<()> {
    if !src.exists() {
        return Err(anyhow!("Source path does not exist"));
    }

    if src.is_dir() {
        if !dst.exists() {
            fs::create_dir_all(dst)?;
        }

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                move_recursive(&src_path, &dst_path)?;
            } else {
                fs::rename(&src_path, &dst_path)?;
            }
        }

        fs::remove_dir(src)?;
    } else {
        fs::rename(src, dst)?;
    }

    Ok(())
}

struct EntryVersion {
    version: Version,
    v_prefixed: bool,
}

impl EntryVersion {
    pub fn from_str(entry: &str) -> Option<Self> {
        let strip = entry.strip_prefix('v');
        let v_prefixed = strip.is_some();

        let unprefixed_entry = strip.unwrap_or(entry);

        if let Ok(version) = Version::parse(unprefixed_entry) {
            return Some(Self {
                version,
                v_prefixed,
            });
        }

        None
    }

    pub fn to_restored(&self) -> String {
        if self.v_prefixed {
            format!("v{}", self.version)
        } else {
            self.version.to_string()
        }
    }
}

impl PartialEq for EntryVersion {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
    }
}

impl Eq for EntryVersion {}

impl PartialOrd for EntryVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EntryVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.version.cmp(&other.version)
    }
}

impl Display for EntryVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.version.fmt(f)
    }
}

fn cleanup_versions() -> Result<()> {
    const KEEP_VERSIONS_AMOUNT: usize = 2;

    let entries = match fs::read_dir(explorer_path()) {
        Ok(entries) => entries,
        Err(err) => return Err(Error::msg(err.to_string())),
    };

    let mut installations: Vec<EntryVersion> = Vec::new();

    for entry in entries {
        let Ok(entry) = entry else { continue };
        let file_name = entry.file_name();
        let entry_name = file_name.to_str().context("no file name on entry")?;

        if let Some(version) = EntryVersion::from_str(entry_name) {
            installations.push(version);
        }
    }

    if installations.is_empty() {
        return Ok(());
    }

    // Sort versions
    installations.sort();

    if installations.len() <= KEEP_VERSIONS_AMOUNT {
        // Don't need to uninstall anything
        return Ok(());
    }

    // Keep the latest 2 versions and delete the rest
    // Arithmetic boundaries are solved on the line above
    #[allow(clippy::arithmetic_side_effects)]
    for version in installations
        .iter()
        .take(installations.len() - KEEP_VERSIONS_AMOUNT)
    {
        let folder_path = explorer_path().join(version.to_restored());
        if folder_path.exists() {
            match fs::remove_dir_all(&folder_path) {
                Ok(()) => log::info!("Removed old version: {}", version),
                Err(err) => log::error!("Failed to remove {}: {}", version, err),
            }
        }
    }

    Ok(())
}

fn is_app_updated(version: &str) -> bool {
    let result = get_version_data();
    match result {
        Ok(data) => {
            if let Some(v) = data.get("version") {
                v == version
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

pub fn is_explorer_installed(version: Option<&str>) -> bool {
    let path = get_explorer_bin_path(version);
    match path {
        Ok(path) => path.exists(),
        Err(_) => false,
    }
}

pub fn is_explorer_updated(version: &str) -> bool {
    is_explorer_installed(Some(version)) && is_app_updated(version)
}

pub fn target_download_path() -> PathBuf {
    explorer_downloads_path().join(EXPLORER_DOWNLOADED_FILENAME)
}

pub fn install_explorer(version: &str, downloaded_file_path: Option<PathBuf>) -> StepResult {
    let branch_path = explorer_path().join(version);
    let file_path = downloaded_file_path
        .unwrap_or_else(|| explorer_downloads_path().join(EXPLORER_DOWNLOADED_FILENAME));

    if !file_path.exists() {
        return StepError::E1001_FILE_NOT_FOUND {
            expected_path: Some(file_path.to_string_lossy().into_owned()),
        }
        .into();
    }

    compression::decompress_file(&file_path, &branch_path)?;

    #[cfg(target_os = "macos")]
    {
        let from = &branch_path.join("build");
        let to = &branch_path;

        move_recursive(from, to).context("Cannot move build folder")?;

        let explorer_bin_path = branch_path.join(EXPLORER_MAC_BIN_PATH);
        if explorer_bin_path.exists() {
            let metadata = fs::metadata(&explorer_bin_path)?;
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(explorer_bin_path, permissions)?;
        }
    }

    let mut version_data = get_version_data_or_empty();

    let install_time = Value::from(
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .context("Cannot convert time")?
            .as_secs()
            .to_string(),
    );
    version_data.insert(version.to_owned(), install_time);

    if version != "dev" {
        version_data.insert("version".to_owned(), Value::String(version.to_owned()));
    }

    version_data.insert("path".to_owned(), branch_path.to_string_lossy().into());

    // Write version data to file
    let version_data_str =
        serde_json::to_string(&version_data).context("Cannot serialize version_data")?;
    let version_path = explorer_version_path();
    fs::write(version_path, version_data_str)?;

    // Remove the downloaded file
    fs::remove_file(file_path)?;
    cleanup_versions().context("Cannot clean up the old versions")?;

    Ok(())
}

pub struct InstallsHub {
    analytics: Arc<Mutex<Analytics>>,
    running_instances: Arc<Mutex<RunningInstances>>,
}

impl InstallsHub {
    pub const fn new(
        analytics: Arc<Mutex<Analytics>>,
        running_instances: Arc<Mutex<RunningInstances>>,
    ) -> Self {
        Self {
            analytics,
            running_instances,
        }
    }

    async fn explorer_params(&self, deeplink: Option<DeepLink>) -> Vec<String> {
        let guard = self.analytics.lock().await;

        let mut output = vec![
            "--launcher_anonymous_id".to_string(),
            guard.anonymous_id().to_owned(),
            "--session_id".to_string(),
            guard.session_id().value().to_owned(),
            "--provider".to_string(),
            AppEnvironment::provider(),
        ];
        drop(guard);

        if let Some(value) = deeplink {
            output.insert(0, value.into());
        }

        output
    }

    fn readable_version(version: Option<&str>) -> String {
        match version {
            Some(v) => v.to_owned(),
            None => {
                let map = get_version_data_or_empty();
                if let Some(v) = map.get("version") {
                    if let Some(str_version) = v.as_str() {
                        return str_version.to_owned();
                    }
                }

                "latest".to_owned()
            }
        }
    }

    async fn send_analytics_event(&self, event: Event) {
        self.analytics
            .lock()
            .await
            .track_and_flush_silent(event)
            .await;
    }

    pub async fn launch_explorer(
        &self,
        deeplink: Option<DeepLink>,
        preferred_version: Option<&str>,
    ) -> Result<()> {
        let readable_version = Self::readable_version(preferred_version);

        self.send_analytics_event(Event::LAUNCH_CLIENT_START {
            version: readable_version.clone(),
        })
        .await;
        let result = self
            .launch_explorer_internal(deeplink, preferred_version)
            .await;
        if let Err(e) = &result {
            self.send_analytics_event(Event::LAUNCH_CLIENT_ERROR {
                version: readable_version,
                error: e.to_string(),
            })
            .await;
        } else {
            self.send_analytics_event(Event::LAUNCH_CLIENT_SUCCESS {
                version: readable_version,
            })
            .await;
        }

        result
    }

    async fn launch_explorer_internal(
        &self,
        deeplink: Option<DeepLink>,
        preferred_version: Option<&str>,
    ) -> Result<()> {
        const WAIT_TIMEOUT: Duration = Duration::from_secs(3);
        const CHECK_INTERVAL: Duration = Duration::from_millis(100);

        log::info!("Launching Explorer...");

        let explorer_bin_path = get_explorer_bin_path(preferred_version)?;
        let explorer_bin_dir = explorer_bin_path
            .parent()
            .ok_or_else(|| anyhow!("Failed to get explorer binary directory"))?;

        if !explorer_bin_path.exists() {
            let error_message = match preferred_version {
                Some(ver) => format!("The explorer version specified ({}) is not installed.", ver),
                None => "The explorer is not installed.".to_string(),
            };
            log::error!("{}, {}", error_message, explorer_bin_path.display());
            return Err(anyhow!(error_message));
        }

        // Ensure binary is executable
        fs::metadata(&explorer_bin_path).context("Failed to access explorer binary")?;

        // Prepare explorer parameters
        let explorer_params = self.explorer_params(deeplink).await;
        log::info!(
            "Opening Explorer at {} with params: {:?}",
            explorer_bin_path.display(),
            explorer_params
        );

        let mut child = Command::new(&explorer_bin_path)
            .current_dir(explorer_bin_dir)
            .args(&explorer_params)
            .detached()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("Failed to start explorer process: {}", e))
            .with_context(|| {
                format!(
                    "Dir: {}, Bin: {} Args: {:?}",
                    explorer_bin_dir.display(),
                    explorer_bin_path.display(),
                    explorer_params
                )
            })?;

        {
            let guard = self.running_instances.lock().await;
            guard.register_instance(child.id());
        }

        #[cfg(windows)]
        let graceful_exit_code: ExitStatus = std::process::ExitStatus::from_raw(0);

        #[cfg(unix)]
        let graceful_exit_code: ExitStatus = ExitStatus::from_raw(0 << 8); // exit code 0

        #[cfg(windows)]
        let still_active_exit_code: ExitStatus = std::process::ExitStatus::from_raw(259);

        // it's clear that CHECK_INTERVAL is never 0 by the const value
        #[allow(clippy::arithmetic_side_effects)]
        for _ in 0..(WAIT_TIMEOUT.as_millis() / CHECK_INTERVAL.as_millis()) {
            if let Some(exit_status) = child.try_wait()? {
                if exit_status == graceful_exit_code {
                    return Ok(());
                }

                #[cfg(windows)]
                if exit_status == still_active_exit_code {
                    break;
                }

                return Err(anyhow!(
                    "Child process died shorly after launch with code: {}",
                    exit_status
                ));
            }

            thread::sleep(CHECK_INTERVAL);
        }

        Ok(())
    }
}
