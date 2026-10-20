//! Where a release's delta set lives in the bucket, and reading its manifest.

use reqwest::StatusCode;

use crate::environment::AppEnvironment;
use crate::errors::{DCLError, DCLErrorTyped};
use crate::s3::RELEASE_PREFIX;
use crate::utils::get_os_name;
use crate::version::Version;

use super::manifest::{BuildPath, MANIFEST_FILE_NAME, Manifest};

/// `<bucket>/<releases>/<tag>/delta/<platform>`: the manifest and its `from/<base>/` patches.
pub fn delta_folder_url(target: &Version) -> String {
    format!(
        "{}/{}/{}/delta/{}",
        AppEnvironment::bucket_url(),
        RELEASE_PREFIX,
        target,
        get_os_name()
    )
}

pub fn manifest_url(target: &Version) -> String {
    format!("{}/{}", delta_folder_url(target), MANIFEST_FILE_NAME)
}

/// `patch` is relative to the manifest's folder, as the manifest states it.
pub fn patch_url(target: &Version, patch: &BuildPath) -> String {
    format!("{}/{}", delta_folder_url(target), patch)
}

/// `None` when the release has no delta set; the launcher then takes the zip.
pub async fn fetch_manifest(target: &Version) -> DCLErrorTyped<Option<Manifest>> {
    let url = manifest_url(target);
    let response =
        reqwest::get(&url)
            .await
            .map_err(|source| DCLError::E4003_DELTA_MANIFEST_UNAVAILABLE {
                url: url.clone(),
                reason: source.to_string(),
            })?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(DCLError::E4003_DELTA_MANIFEST_UNAVAILABLE {
            url,
            reason: format!("HTTP {}", response.status()),
        });
    }
    let json =
        response
            .text()
            .await
            .map_err(|source| DCLError::E4003_DELTA_MANIFEST_UNAVAILABLE {
                url: url.clone(),
                reason: source.to_string(),
            })?;
    let manifest = Manifest::parse(&json, &target.to_string()).map_err(|source| {
        DCLError::E4004_DELTA_MANIFEST_INVALID {
            url,
            reason: source.to_string(),
        }
    })?;
    Ok(Some(manifest))
}
