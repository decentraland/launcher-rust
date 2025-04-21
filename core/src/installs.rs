use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use std::thread;
use std::{fs, env, fs::create_dir_all};
use std::path::{Path, PathBuf};
use serde_json::{Map, Value};
use anyhow::{anyhow, Context, Error, Result};
use semver::Version;
use tokio::sync::Mutex;
use crate::analytics::Analytics;
use crate::processes::CommandExtDetached;
use crate::utils;
use crate::environment::AppEnvironment;
use crate::protocols::Protocol;
use crate::analytics::event::Event;

#[cfg(target_os = "macos")]
use std::os::unix::fs::PermissionsExt;

pub mod downloads;
pub mod compression;

const APP_NAME: &str = "DecentralandLauncherLight";
const EXPLORER_DOWNLOADED_FILENAME: &str = "decentraland.zip";
const EXPLORER_MAC_BIN_PATH: &str = "Decentraland.app/Contents/MacOS/Explorer";
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
        let dir = env::var("APPDATA")?;
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

fn get_app_base_path() -> PathBuf {
    dirs::data_local_dir()
        .expect("Failed to get current directory")
}

fn explorer_path() -> PathBuf {
    get_app_base_path().join(APP_NAME)
}

fn explorer_downloads_path() -> PathBuf {
    let dir = explorer_path().join("downloads");
    create_dir_all(&dir).expect("Cannot create downloads directory");
    dir 
}

fn explorer_version_path() -> PathBuf {
    explorer_path().join("version.json")
}

fn explorer_latest_version_path() -> Result<PathBuf> {
    let data  = get_version_data()?;
    let path = &data["path"];
    let value = path.as_str().context("cannot get string value from path property")?;
    Ok(PathBuf::from(value))
}

fn explorer_dev_version_path() -> PathBuf {
    explorer_path().join("dev")
}

fn get_version_data() -> Result<Value> {
    let path = explorer_version_path();
    if path.exists() {
        let data = fs::read_to_string(path).context("Failed to read version.json")?;
        return serde_json::from_str::<serde_json::Value>(&data).context("Failed to parse JSON");
    } 

    Err(anyhow!(format!("File doesn't exists: {}", path.to_str().unwrap_or("no path"))))
}

fn get_version_data_or_empty() -> Value {
    get_version_data().unwrap_or_else(|_| Value::Object(Map::new()))
}

#[cfg(target_os = "macos")]
fn get_explorer_bin_path(version: Option<&str>) -> PathBuf {
    let base_path = match version {
        Some("dev") => explorer_dev_version_path(),
        Some(v) => explorer_path().join(v),
        None => explorer_latest_version_path(),
    };
    base_path.join(EXPLORER_MAC_BIN_PATH)
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

async fn cleanup_versions() -> Result<()> {
    let entries = match fs::read_dir(explorer_path()) {
        Ok(entries) => entries,
        Err(err) => return Err(Error::msg(err.to_string())),
    };

    let mut installations: Vec<Version> = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let file_name = entry.file_name();
        let entry_name = file_name.to_str().context("no file name on entry")?;
        
        if let Ok(version) = Version::parse(&entry_name) {
            installations.push(version);
        } 
    }

    if installations.is_empty() {
        return Ok(());
    }

    // Sort versions
    installations.sort_by(|a, b| a.cmp(&b));

    // Keep the latest 2 versions and delete the rest
    for version in installations.iter().take(installations.len() - 2) {
        let folder_path = explorer_path().join(version.to_string());
        if folder_path.exists() {
            match fs::remove_dir_all(&folder_path) {
                Ok(_) => println!("Removed old version: {}", version),
                Err(err) => eprintln!("Failed to remove {}: {}", version, err),
            }
        }
    }

    Ok(())
}

fn is_app_updated(version: &str) -> bool {
    let result = get_version_data();
    match result {
        Ok(data) => {
            data["version"] == version
        },
        Err(_) => {
            false
        },
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

pub async fn install_explorer(version: &str, downloaded_file_path: Option<PathBuf>) -> Result<()> {
    let branch_path = explorer_path().join(version);
    let file_path = downloaded_file_path.unwrap_or_else(|| explorer_downloads_path().join(EXPLORER_DOWNLOADED_FILENAME));

    if !file_path.exists() {
        return Err(anyhow!(format!("Downloaded explorer file not found: {}", file_path.to_string_lossy())));
    }

    compression::decompress_file(&file_path, &branch_path)
        .map_err(|e| anyhow::Error::msg(format!("Cannot decompress file {}", e.to_string())))?;

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
    version_data[version] = Value::from(std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH)?.as_secs().to_string());
    if version != "dev" {
        version_data["version"] = Value::String(version.to_string());
    }

    version_data["path"] = branch_path.to_string_lossy().into();

    // Write version data to file
    let version_data_str = serde_json::to_string(&version_data)?;
    let version_path = explorer_version_path();
    fs::write(version_path, version_data_str).context("Cannot write version data")?;

    // Remove the downloaded file
    fs::remove_file(file_path).context("Cannot remove the downloaded file")?;
    cleanup_versions().await.context("Cannot clean up the old versions")?;

    Ok(())
}

pub struct InstallsHub {
   analytics: Arc<Mutex<Analytics>>,
}

impl InstallsHub {

    pub fn new(analytics: Arc<Mutex<Analytics>>) -> Self {
        InstallsHub {
            analytics,
        }
    }

    async fn explorer_params(&self) -> Vec<String> {
        let guard = self.analytics.lock().await;

        let mut output = vec![
            "--launcher_anonymous_id".to_string(),
            guard.anonymous_id().to_owned(),
            "--session_id".to_string(),
            guard.session_id().value().to_owned(),
            "--provider".to_string(),
            AppEnvironment::provider(),
        ];

        if let Some(value) = Protocol::value() {
            output.insert(0, value);
        }

        output
    }

    fn readable_version(version: Option<&str>) -> String {
        match version {
            Some(v) => {v.to_owned()},
            None => {
                let result = get_version_data_or_empty();
                if let Some(map) = result.as_object() {
                    if let Some(v) = map.get("version") {
                        if let Some(str_version) = v.as_str() {
                            return str_version.to_owned()
                        }
                    }

                }
            
                "latest".to_owned()
            },
        }
    }

    async fn send_analytics_event(&self, event: Event) -> Result<()> {
        let mut guard = self.analytics.lock().await;
        guard.track_and_flush(event).await
    }

    pub async fn launch_explorer(&self, preferred_version: Option<&str>) -> Result<()> {
        let readable_version = InstallsHub::readable_version(preferred_version.clone());

        self.send_analytics_event(Event::LAUNCH_CLIENT_START { version: readable_version.clone() }).await?;
        let result = self.launch_explorer_internal(preferred_version).await;
        if let Err(e) = &result {
            self.send_analytics_event(Event::LAUNCH_CLIENT_ERROR { version: readable_version, error: e.to_string() }).await?;
        }
        else {
            self.send_analytics_event(Event::LAUNCH_CLIENT_SUCCESS { version: readable_version }).await?;
        }

        result
    }

    async fn launch_explorer_internal(&self, preferred_version: Option<&str>) -> Result<()> {
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
            log::error!("{}, {:?}", error_message, explorer_bin_path);
            return Err(anyhow!(error_message));
        }

        // Ensure binary is executable
        fs::metadata(&explorer_bin_path)
            .context("Failed to access explorer binary")?;

        // Prepare explorer parameters
        let explorer_params = self.explorer_params().await;
        log::info!("Opening Explorer at {:?} with params: {:?}", explorer_bin_path, explorer_params);

        let mut child = Command::new(&explorer_bin_path)
            .current_dir(&explorer_bin_dir)
            .args(&explorer_params)
            .detached()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("Failed to start explorer process: {}", e))
            .with_context(|| format!("Dir: {:?}, Bin: {:?} Args: {:?}", explorer_bin_dir, explorer_bin_path, explorer_params))?;

        log::info!("Process run with id: {}", child.id());

        // TODO make with for loop with separations;
        const WAIT_TIMEOUT: Duration = Duration::from_secs(10);
        thread::sleep(WAIT_TIMEOUT);
        let exit_code = child.try_wait()?;
        if let Some(exit_status) = exit_code {
            return Err(anyhow!("Child process exited with code: {}", exit_status));
        }

        const ALIVE_TIMEOUT: Duration = Duration::from_secs(2);
        thread::sleep(ALIVE_TIMEOUT);
        let exit_code = child.try_wait()?;
        if let Some(exit_status) = exit_code {
            return Err(anyhow!("Process died shorly after its start with code: {}", exit_status));
        }

        Ok(())
    }
}
