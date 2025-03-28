use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::{fs, env, fs::create_dir_all};
use std::path::{Path, PathBuf};
use serde_json::{Map, Value};
use anyhow::{anyhow, Context, Result, Error};
use semver::Version;
use tokio::sync::Mutex;
use crate::analytics::Analytics;
use crate::processes::CommandExtDetached;
use crate::utils;
use crate::environment::AppEnvironment;
use crate::protocols::Protocol;

pub mod downloads;
pub mod compression;

const APP_NAME: &str = "DecentralandLauncherLight";
const EXPLORER_DOWNLOADED_FILENAME: &str = "decentraland.zip";
const EXPLORER_MAC_BIN_PATH: &str = "Decentraland.app/Contents/MacOS/Explorer";
const EXPLORER_WIN_BIN_PATH: &str = "Decentraland.exe";

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

fn explorer_latest_version_path() -> PathBuf {
    explorer_path().join("latest")
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
fn get_explorer_bin_path(version: Option<&str>) -> PathBuf {
    let base_path = match version {
        Some("dev") => explorer_dev_version_path(),
        Some(v) => explorer_path().join(v),
        None => explorer_latest_version_path(),
    };
    base_path.join(EXPLORER_WIN_BIN_PATH)
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
    get_explorer_bin_path(version).exists()
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

    if utils::get_os_name() == "macos" {

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

    // Remove old version symlink if it exists
    let latest_path = explorer_latest_version_path();
    if latest_path.exists() {
        fs::remove_file(&latest_path).context("Cannot delete latest version")?;
    }

    // Create a symlink
    // TODO support on windows
    std::os::unix::fs::symlink(&branch_path, latest_path).context("Cannot create symlink")?;

    let mut version_data = get_version_data_or_empty();
    version_data[version] = Value::from(std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH)?.as_secs().to_string());
    if version != "dev" {
        version_data["version"] = Value::String(version.to_string());
    }

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


    pub async fn launch_explorer(&self, preferred_version: Option<&str>) -> Result<()> {
        log::info!("Launching Explorer...");

        let explorer_bin_path = get_explorer_bin_path(preferred_version);
        let explorer_bin_dir = explorer_bin_path
            .parent()
            .ok_or_else(|| anyhow!("Failed to get explorer binary directory"))?;

        if !explorer_bin_path.exists() {
            let error_message = match preferred_version {
                Some(ver) => format!("The explorer version specified ({}) is not installed.", ver),
                None => "The explorer is not installed.".to_string(),
            };
            log::error!("{}, {}", error_message, explorer_bin_path.to_string_lossy().to_string());
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
            .context("Failed to start explorer process")?;

        log::info!("Process run with id: {}", child.id());

        std::thread::sleep(std::time::Duration::from_secs(60*10));
        //child.wait()?;

        Ok(())
    }
}
