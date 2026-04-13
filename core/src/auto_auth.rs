pub mod anon_user_id;
pub mod auth_token_storage;

use anyhow::{Result, anyhow};
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

use auth_token_storage::AuthTokenStorage;

/// Data extracted from the installer's download origin (xattr URLs on macOS,
/// Zone.Identifier on Windows).
pub struct DownloadOriginData {
    pub auth_token: Option<String>,
    pub campaign_anon_user_id: Option<String>,
}

pub struct AutoAuth {}

impl AutoAuth {
    pub fn try_obtain_auth_token() {
        let has_token = AuthTokenStorage::has_token();
        let has_anon_id = crate::config::campaign_anon_user_id().is_some();

        if has_token {
            log::info!("Token already obtained");
        }

        // On macOS, skip extraction only when BOTH are already present
        #[cfg(target_os = "macos")]
        if has_token && has_anon_id {
            return;
        }

        // On Windows, token extraction is handled by src-auto-auth binary;
        // only skip if token exists (anon_user_id is also handled there).
        #[cfg(not(target_os = "macos"))]
        if has_token {
            return;
        }

        #[cfg(target_os = "macos")]
        match Self::obtain_token_internal() {
            Ok(origin) => {
                // Handle auth token
                if !has_token {
                    match origin.auth_token {
                        Some(token) => {
                            log::info!("Token obtained");
                            if let Err(e) = AuthTokenStorage::write_token(token.as_str()) {
                                log::error!("Cannot write token: {e}");
                            }
                        }
                        None => {
                            log::warn!("Token value is empty");
                        }
                    }
                }

                // Handle anon_user_id independently of token
                if !has_anon_id {
                    if let Some(ref anon_id) = origin.campaign_anon_user_id {
                        log::info!("Campaign anon_user_id obtained from DMG origin");
                        crate::config::write_campaign_anon_user_id(anon_id);
                    }
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

    /// Extracts auth token and campaign `anon_user_id` from the DMG's
    /// `where-from` xattr URLs.  Both fields are independent — an `anon_user_id`
    /// can be present even when no auth token is found.
    #[cfg(target_os = "macos")]
    fn obtain_token_internal() -> Result<DownloadOriginData> {
        use anyhow::Context;

        use crate::environment::macos::{dmg_backing_file, dmg_mount_path, where_from_attr};

        let path = std::env::current_exe()?;
        log::info!("Exe path: {}", path.display());
        let dmg_mount_path = dmg_mount_path(&path)?;
        log::info!("Exe is running from dmg: {dmg_mount_path:?}");

        let Some(dmg_mount_path) = dmg_mount_path else {
            return Ok(DownloadOriginData {
                auth_token: None,
                campaign_anon_user_id: None,
            });
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

        let mut found_token: Option<String> = None;
        let mut found_anon_user_id: Option<String> = None;

        for attr in &where_from {
            // Extract auth token
            if found_token.is_none() {
                match token_from_url(attr) {
                    Ok(token) => {
                        if token.is_some() {
                            found_token = token;
                        }
                    }
                    Err(e) => {
                        log::error!("Cannot read token from url '{}' due: {}", attr, e);
                    }
                }
            }

            // Extract anon_user_id (independently)
            if found_anon_user_id.is_none() {
                if let Some(anon_id) = anon_user_id::anon_user_id_from_url(attr) {
                    found_anon_user_id = Some(anon_id);
                }
            }
        }

        Ok(DownloadOriginData {
            auth_token: found_token,
            campaign_anon_user_id: found_anon_user_id,
        })
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

    // Search in params — skip anon_user_id to avoid treating it as an auth token
    for (key, value) in url.query_pairs() {
        if key == "anon_user_id" {
            continue;
        }
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
