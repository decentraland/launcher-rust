use std::os::unix::fs::PermissionsExt;
use std::{fs, env};
use std::path::{Path, PathBuf};
use serde_json::Value;
use anyhow::{anyhow, Context, Result, Error};
use semver::Version;

use crate::utils;

pub mod downloads;
pub mod compression;

const EXPLORER_DOWNLOADED_FILENAME: &str = "decentraland.zip";
const EXPLORER_MAC_BIN_PATH: &str = "/Decentraland.app/Contents/MacOS/Explorer";
const EXPLORER_WIN_BIN_PATH: &str = "/Decentraland.exe";

fn get_app_base_path() -> PathBuf {
    env::current_dir().expect("Failed to get current directory")
}

fn explorer_path() -> PathBuf {
    get_app_base_path().join("Explorer")
}

fn explorer_downloads_path() -> PathBuf {
    explorer_path().join("downloads")
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

fn get_version_data() -> Result<serde_json::Value> {
    let path = explorer_version_path();
    if path.exists() {
        let data = fs::read_to_string(path).context("Failed to read version.json")?;
        return serde_json::from_str::<serde_json::Value>(&data).context("Failed to parse JSON");
    } 

    Err(anyhow!(format!("File doesn't exists: {}", path.to_str().unwrap_or("no path"))))
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

fn is_app_updated(app_path: &Path, version: &str) -> bool {
    let version_file = app_path.join("version.json");
    if let Ok(data) = fs::read_to_string(version_file) {
        if let Ok(version_data) = serde_json::from_str::<serde_json::Value>(&data) {
            return version_data["version"] == version;
        }
    }
    false
}

pub fn is_explorer_installed(version: Option<&str>) -> bool {
    get_explorer_bin_path(version).exists()
}

pub fn is_explorer_updated(version: &str) -> bool {
    is_explorer_installed(Some(version)) && is_app_updated(&explorer_path(), version)
}

pub fn target_download_path() -> PathBuf {
    explorer_downloads_path().join(EXPLORER_DOWNLOADED_FILENAME)
}

pub async fn install_explorer(version: &str, downloaded_file_path: Option<PathBuf>) -> Result<()> {
    let branch_path = explorer_path().join(version);
    let file_path = downloaded_file_path.unwrap_or_else(|| explorer_downloads_path().join(EXPLORER_DOWNLOADED_FILENAME));

    let mut version_data = get_version_data()?;

    if !file_path.exists() {
        return Err(anyhow!(format!("Downloaded explorer file not found: {}", file_path.to_string_lossy())));
    }

    compression::decompress_file(&file_path, &branch_path)
        .map_err(|e| anyhow::Error::msg(format!("Cannot decompress file {}", e.to_string())))?;

    if utils::get_os_name() == "macos" {
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
        fs::remove_file(&latest_path)?;
    }

    // Create a symlink
    // TODO support on windows
    std::os::unix::fs::symlink(&branch_path, latest_path)?;

    version_data[version] = Value::from(std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH)?.as_secs().to_string());
    if version != "dev" {
        version_data["version"] = Value::String(version.to_string());
    }

    // Write version data to file
    let version_data_str = serde_json::to_string(&version_data)?;
    let version_path = explorer_version_path();
    fs::write(version_path, version_data_str)?;

    // Remove the downloaded file
    fs::remove_file(file_path)?;
    cleanup_versions().await?;

    Ok(())
}
