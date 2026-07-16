pub mod anon_user_id;
pub mod auth_token_storage;
pub mod campaign_anon_user_id_storage;
pub mod campaign_attribution_marker;
pub mod startup_location_storage;

#[cfg(target_os = "macos")]
use anyhow::anyhow;
use anyhow::Result;
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use auth_token_storage::AuthTokenStorage;

use url::form_urlencoded;

use crate::protocols::DeepLink;
#[cfg(target_os = "macos")]
use crate::protocols::Protocol;

use anon_user_id::AnonUserId;
#[cfg(target_os = "macos")]
use campaign_anon_user_id_storage::CampaignAnonUserIdStorage;
#[cfg(target_os = "macos")]
use startup_location_storage::StartupLocationStorage;

/// Data extracted from a download URL — auth token, campaign anonymous user ID,
/// and optional startup deeplink position/realm.
///
/// All fields are parsed independently from the same URL via `from_url()`,
/// avoiding ordering or collision issues between them.
#[derive(Default)]
pub struct DownloadOriginData {
    pub auth_token: Option<String>,
    pub campaign_anon_user_id: Option<AnonUserId>,
    pub startup_position: Option<String>,
    pub startup_realm: Option<String>,
}

impl DownloadOriginData {
    /// Extract both auth token and `anon_user_id` from a single URL.
    ///
    /// The auth token is matched by UUID regex on any query param value (except
    /// `anon_user_id`) or path segment. The `anon_user_id` is matched by key name.
    pub fn from_url(url_str: &str) -> Result<Self> {
        let url = url::Url::parse(url_str)?;

        let re = regex::Regex::new(
            r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        )?;

        let mut auth_token: Option<String> = None;
        let mut startup_position: Option<String> = None;
        let mut startup_realm: Option<String> = None;

        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "anon_user_id" => {}
                "position" if !value.is_empty() => {
                    startup_position = Some(value.to_string());
                }
                "realm" if !value.is_empty() => {
                    startup_realm = Some(value.to_string());
                }
                _ => {
                    if auth_token.is_none() && re.is_match(&value) {
                        auth_token = Some(value.to_string());
                    }
                }
            }
        }

        if auth_token.is_none() {
            if let Some(segments) = url.path_segments() {
                auth_token = segments
                    .filter(|s| re.is_match(s))
                    .map(ToString::to_string)
                    .next();
            }
        }

        let campaign_anon_user_id = AnonUserId::from_url(url_str);

        Ok(Self {
            auth_token,
            campaign_anon_user_id,
            startup_position,
            startup_realm,
        })
    }

    /// Builds a `decentraland://` startup deeplink from whichever of
    /// `position`/`realm` are present. Returns `None` when neither is set, so a
    /// missing field never produces an empty or malformed deeplink.
    ///
    /// Shared by the macOS (xattr) and Windows (`Zone.Identifier`) flows so both
    /// platforms handle position-only, realm-only, and both-present identically.
    pub fn to_startup_deeplink(&self) -> Option<DeepLink> {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        if let Some(ref position) = self.startup_position {
            serializer.append_pair("position", position);
        }
        if let Some(ref realm) = self.startup_realm {
            serializer.append_pair("realm", realm);
        }
        let query = serializer.finish();
        if query.is_empty() {
            return None;
        }
        Some(DeepLink::from_query(&query))
    }
}

pub struct DownloadOrigin {}

impl DownloadOrigin {
    /// Extracts auth token, campaign `anon_user_id`, and startup deeplink
    /// (position/realm) from the DMG's xattr URLs.
    ///
    /// Windows is handled by the `src-auto-auth` binary, so this is macOS-only.
    #[cfg(target_os = "macos")]
    pub fn try_extract_origin_data() {
        Self::try_extract_from_dmg();
    }

    #[cfg(target_os = "macos")]
    fn try_extract_from_dmg() {
        let has_token = AuthTokenStorage::has_token();
        let has_anon_id = CampaignAnonUserIdStorage::has();

        if has_token {
            log::info!("Token already obtained");
        }

        match Self::obtain_token_internal() {
            Ok(origin) => {
                if !has_token {
                    match origin.auth_token.as_ref() {
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

                if !has_anon_id {
                    if let Some(ref anon_id) = origin.campaign_anon_user_id {
                        log::info!("Campaign anon_user_id obtained from DMG origin");
                        if let Err(e) = CampaignAnonUserIdStorage::write(anon_id) {
                            log::error!("Cannot write campaign anon user id: {e}");
                        }
                    }
                }

                if !StartupLocationStorage::has() {
                    if let Some(deeplink) = origin.to_startup_deeplink() {
                        log::info!("Seeding startup location deeplink: {}", deeplink.original());
                        Protocol::store(deeplink);
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
            return Ok(DownloadOriginData::default());
        };

        let dmg_file_path = dmg_backing_file(&dmg_mount_path.to_string_lossy())
            .with_context(|| format!("Cannot resolve mount path: {}", dmg_mount_path.display()))?
            .ok_or_else(|| anyhow!("Dmg original file not found: {}", dmg_mount_path.display()))?;
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

        let mut result = DownloadOriginData::default();

        for attr in &where_from {
            match DownloadOriginData::from_url(attr) {
                Ok(parsed) => {
                    result.auth_token = result.auth_token.or(parsed.auth_token);
                    result.campaign_anon_user_id =
                        result.campaign_anon_user_id.or(parsed.campaign_anon_user_id);
                    result.startup_position = result.startup_position.or(parsed.startup_position);
                    result.startup_realm = result.startup_realm.or(parsed.startup_realm);
                }
                Err(e) => {
                    log::error!("Cannot parse url '{}': {}", attr, e);
                }
            }
        }

        Ok(result)
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
    fn test_token_from_url(#[case] url: &str, #[case] expected_token: &str) -> anyhow::Result<()> {
        let origin = DownloadOriginData::from_url(url)?;
        let token = origin.auth_token.ok_or_else(|| anyhow!("No token found"))?;
        assert_eq!(expected_token, token.as_str());
        Ok(())
    }

    #[test]
    fn test_both_token_and_anon_id() -> anyhow::Result<()> {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg?anon_user_id=abc-123";
        let origin = DownloadOriginData::from_url(url)?;
        assert_eq!(
            origin.auth_token.as_deref(),
            Some("391a85da-a3bb-49e2-a45e-96c740c38424")
        );
        assert!(origin.campaign_anon_user_id.is_some());
        assert_eq!(
            origin.campaign_anon_user_id.map(|id| id.as_str().to_owned()),
            Some("abc-123".to_owned())
        );
        Ok(())
    }

    #[test]
    fn test_position_and_realm_extracted() -> anyhow::Result<()> {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg?position=42%2C-5&realm=myworld.dcl.eth";
        let origin = DownloadOriginData::from_url(url)?;
        assert_eq!(origin.startup_position.as_deref(), Some("42,-5"));
        assert_eq!(origin.startup_realm.as_deref(), Some("myworld.dcl.eth"));
        Ok(())
    }

    #[test]
    fn test_position_without_realm() -> anyhow::Result<()> {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg?position=10%2C20";
        let origin = DownloadOriginData::from_url(url)?;
        assert_eq!(origin.startup_position.as_deref(), Some("10,20"));
        assert!(origin.startup_realm.is_none());
        Ok(())
    }

    #[test]
    fn test_all_fields_together() -> anyhow::Result<()> {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg?anon_user_id=abc-123&position=5%2C10&realm=dragon.dcl.eth";
        let origin = DownloadOriginData::from_url(url)?;
        assert_eq!(
            origin.auth_token.as_deref(),
            Some("391a85da-a3bb-49e2-a45e-96c740c38424")
        );
        assert_eq!(
            origin.campaign_anon_user_id.map(|id| id.as_str().to_owned()),
            Some("abc-123".to_owned())
        );
        assert_eq!(origin.startup_position.as_deref(), Some("5,10"));
        assert_eq!(origin.startup_realm.as_deref(), Some("dragon.dcl.eth"));
        Ok(())
    }

    #[test]
    fn test_no_position_no_realm() -> anyhow::Result<()> {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg";
        let origin = DownloadOriginData::from_url(url)?;
        assert!(origin.startup_position.is_none());
        assert!(origin.startup_realm.is_none());
        Ok(())
    }

    #[test]
    fn test_build_deeplink_position_and_realm() {
        let origin = DownloadOriginData {
            startup_position: Some("42,-5".to_owned()),
            startup_realm: Some("myworld.dcl.eth".to_owned()),
            ..DownloadOriginData::default()
        };
        assert_eq!(
            origin.to_startup_deeplink().map(String::from).as_deref(),
            Some("decentraland://position=42%2C-5&realm=myworld.dcl.eth")
        );
    }

    #[test]
    fn test_build_deeplink_position_only() {
        let origin = DownloadOriginData {
            startup_position: Some("100,100".to_owned()),
            ..DownloadOriginData::default()
        };
        assert_eq!(
            origin.to_startup_deeplink().map(String::from).as_deref(),
            Some("decentraland://position=100%2C100")
        );
    }

    #[test]
    fn test_build_deeplink_realm_only() {
        let origin = DownloadOriginData {
            startup_realm: Some("eax.dcl.eth".to_owned()),
            ..DownloadOriginData::default()
        };
        assert_eq!(
            origin.to_startup_deeplink().map(String::from).as_deref(),
            Some("decentraland://realm=eax.dcl.eth")
        );
    }

    #[test]
    fn test_build_deeplink_neither() {
        let origin = DownloadOriginData::default();
        assert!(origin.to_startup_deeplink().is_none());
    }

    /// A crafted `position` containing `&`/`=` must not smuggle in an extra
    /// deeplink arg: it is percent-encoded and stays a single `position` value.
    #[test]
    fn test_build_deeplink_neutralizes_injection() {
        let origin = DownloadOriginData {
            startup_position: Some("0,0&local-scene=true".to_owned()),
            ..DownloadOriginData::default()
        };
        let deeplink = origin.to_startup_deeplink().expect("deeplink");
        assert_eq!(
            deeplink.original(),
            "decentraland://position=0%2C0%26local-scene%3Dtrue"
        );
        // The injected pair is not promoted to a first-class deeplink arg.
        assert!(!deeplink.has_true_value("local-scene"));
    }
}
