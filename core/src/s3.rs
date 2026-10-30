use reqwest;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::environment::{AppEnvironment, Args};
use crate::errors::{DCLError, DCLErrorTyped};
use crate::utils::get_os_name;

pub const RELEASE_PREFIX: &str = "@dcl/unity-explorer/releases";

#[derive(Deserialize, Debug)]
struct LatestRelease {
    version: String,
}

#[derive(Deserialize, Debug)]
pub struct ReleaseResponse {
    pub browser_download_url: String,
    pub version: String,
}

fn latest_json_url() -> String {
    let args: Args = AppEnvironment::cmd_args();
    if let Some(url) = args.use_latest_json_url {
        return url;
    }

    let bucket_url = AppEnvironment::bucket_url();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!(
        "{}/{}/latest.json?_t={}",
        bucket_url, RELEASE_PREFIX, timestamp
    )
}

async fn fetch_explorer_latest_release() -> DCLErrorTyped<LatestRelease> {
    let url = latest_json_url();
    log::info!(
        "[fetch_explorer_latest_release] Fetching latest release from: {}",
        url
    );

    let response = reqwest::get(&url).await?;

    if !response.status().is_success() {
        return DCLError::E2004_DOWNLOAD_FAILED_HTTP_CODE {
            url,
            code: response.status().into(),
        }
        .into();
    }

    let data = response.json::<LatestRelease>().await?;
    log::info!(
        "[fetch_explorer_latest_release] Latest release fetched successfully: {:?}",
        data
    );

    Ok(data)
}

pub fn release_zip_url(version: &str) -> String {
    format!(
        "{}/{}/{}/Decentraland_{}.zip",
        AppEnvironment::bucket_url(),
        RELEASE_PREFIX,
        version,
        get_os_name()
    )
}

pub async fn get_latest_explorer_release() -> DCLErrorTyped<ReleaseResponse> {
    let latest_release = fetch_explorer_latest_release().await?;
    let release_url = release_zip_url(&latest_release.version);

    log::info!(
        "[get_latest_explorer_release] Release URL generated: {{ os: {}, version: {}, url: {} }}",
        get_os_name(),
        latest_release.version,
        release_url
    );

    let response = ReleaseResponse {
        browser_download_url: release_url,
        version: latest_release.version,
    };

    Ok(response)
}
