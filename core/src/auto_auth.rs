pub mod auth_token_storage;

use anyhow::{Result, anyhow};
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

use auth_token_storage::AuthTokenStorage;

pub struct AutoAuth {}

impl AutoAuth {
    pub fn try_obtain_auth_token() {
        if AuthTokenStorage::has_token() {
            log::info!("Token already obtained");

            // No need to return on windows, just check if token obtained
            #[cfg(target_os = "macos")]
            return;
        }

        #[cfg(target_os = "macos")]
        match Self::obtain_token_internal() {
            Ok(token) => {
                let Some(token) = token else {
                    log::warn!("Token value is empty");
                    return;
                };

                log::info!("Token obtained");
                if let Err(e) = AuthTokenStorage::write_token(token.as_str()) {
                    log::error!("Cannot write token: {e}");
                }
            }
            Err(e) => {
                log::error!("Obtain auth token error: {e}");
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn try_install_to_app_dir_if_from_dmg() {
        log::info!("Auto install attempt begin");
        if let Err(e) = Self::install_to_app_dir_if_from_dmg() {
            log::error!("Cannot auto install from dmg: {}", e);
        } else {
            log::info!("Auto install attempt complete");
        }
    }

    #[cfg(target_os = "macos")]
    fn install_to_app_dir_if_from_dmg() -> Result<()> {
        let from_dmg = crate::environment::macos::is_running_from_dmg()?;

        if !from_dmg {
            log::info!("App is not running from dmg, no copying needed");
            return Ok(());
        }

        let exe_path = std::env::current_exe()?;
        let app_bundle = app_bundle_from_exe_path(&exe_path)?;
        let app_name = app_bundle
            .file_name()
            .ok_or_else(|| anyhow!("Cannot get name from app bundle"))?;
        let dest_path = PathBuf::from("/Applications").join(app_name);

        if dest_path.exists() {
            log::info!("App is already in /Applications, skipping copying from dmg");
            return Ok(());
        }

        log::info!(
            "Copying app bundle from {} to {}",
            app_bundle.display(),
            dest_path.display()
        );

        // Use Apple's ditto, safest and signature-preserving
        let status = std::process::Command::new("ditto")
            .arg(&app_bundle)
            .arg(&dest_path)
            .status()?;

        if !status.success() {
            return Err(anyhow!("ditto failed to copy the app bundle"));
        }

        log::info!("Copy successful");
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn obtain_token_internal() -> Result<Option<String>> {
        use anyhow::Context;

        use crate::environment::macos::{dmg_backing_file, dmg_mount_path, where_from_attr};

        let path = std::env::current_exe()?;
        log::info!("Exe path: {}", path.display());
        let dmg_mount_path = dmg_mount_path(&path)?;
        log::info!("Exe is running from dmg: {dmg_mount_path:?}");

        let Some(dmg_mount_path) = dmg_mount_path else {
            return Ok(None);
        };

        let dmg_file_path = dmg_backing_file(&dmg_mount_path.to_string_lossy())
            .with_context(|| format!("Cannot resolve mount path: {}", dmg_mount_path.display()))?
            .ok_or_else(|| anyhow!("Dmg original file not found: {dmg_mount_path:?}"))?;
        let where_from = where_from_attr(dmg_file_path.as_path())
            .with_context(|| "Cannot read where from attr: {dmg_file_path}")?;

        log::info!(
            "Where from attr: {:?} for path: {}",
            where_from,
            dmg_file_path.display()
        );

        let Some(where_from) = where_from else {
            return Err(anyhow!("Dmg does not have where from data"));
        };

        for attr in &where_from {
            let url = token_from_url(attr);
            match url {
                Ok(token) => {
                    if token.is_some() {
                        return Ok(token);
                    }
                }
                Err(e) => {
                    log::error!("Cannot read url '{}' due: {}", attr, e);
                }
            }
        }

        Ok(None)
    }
}

#[cfg(target_os = "macos")]
fn app_bundle_from_exe_path(exe_path: &Path) -> std::io::Result<PathBuf> {
    let mut path = exe_path.to_path_buf();
    while let Some(parent) = path.parent() {
        if parent.extension().is_some_and(|e| e == "app") {
            return Ok(parent.to_path_buf());
        }
        path = parent.to_path_buf();
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "App bundle not found",
    ))
}

pub fn token_from_url(url_str: &str) -> Result<Option<String>> {
    let url = url::Url::parse(url_str)?;

    // Regex for token find
    let re = regex::Regex::new(
        r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
    )?;

    // Search in params
    for (_, value) in url.query_pairs() {
        if re.is_match(&value) {
            return Ok(Some(value.to_string()));
        }
    }

    // Split into path segments e.g. "391a85da-a3bb-49e2-a45e-96c740c38424"
    let mut segments = url
        .path_segments()
        .ok_or_else(|| anyhow!("Cannot split url"))?;

    Ok(segments.find(|s| re.is_match(s)).map(ToString::to_string))
}

#[cfg(target_os = "macos")]
#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(
        "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg",
        "391a85da-a3bb-49e2-a45e-96c740c38424"
    )]
    #[case(
        "https://explorer-artifacts.decentraland.zone/dry-run-launcher-rust/pr-196/run-855-19672401394/Decentraland_installer.exe?token=b5876cf1-9b6b-451e-b467-9700f754a8f7",
        "b5876cf1-9b6b-451e-b467-9700f754a8f7"
    )]
    fn test_token_from_url(#[case] url: &str, #[case] expected_token: &str) -> Result<()> {
        let token = token_from_url(url)?.ok_or_else(|| anyhow!("Empty url"))?;
        assert_eq!(expected_token, token.as_str());
        Ok(())
    }
}
