use reqwest;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::canary;
use crate::environment::{AppEnvironment, Args};
use crate::errors::{DCLError, DCLResultTyped};
use crate::installs;
use crate::utils::get_os_name;

pub const RELEASE_PREFIX: &str = "@dcl/unity-explorer/releases";

const CANARY_JSON_NAME: &str = "canary.json";

#[derive(Deserialize, Debug)]
struct LatestRelease {
    version: String,
}

/// Which release track the launcher resolved for this run. Reported to analytics so the
/// canary/regular split can be measured per install.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseChannel {
    /// The regular `latest.json` release everybody gets.
    #[default]
    Latest,
    /// The canary (nightly/experimental) build, served from its own url.
    Canary,
}

impl ReleaseChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::Canary => "canary",
        }
    }
}

#[derive(Debug)]
pub struct ReleaseResponse {
    pub browser_download_url: String,
    pub version: String,
    pub channel: ReleaseChannel,
}

/// `canary.json` - the only piece of canary configuration that lives in the backend: where the
/// build is and what share of new users should get it.
#[derive(Deserialize, Debug)]
pub struct CanaryReleaseResponse {
    pub browser_download_url: String,
    pub version: String,
    pub cohort: u8, // range 0..100 - 100 is max and means ALL users under canary
}

impl From<CanaryReleaseResponse> for ReleaseResponse {
    fn from(value: CanaryReleaseResponse) -> Self {
        Self {
            browser_download_url: value.browser_download_url,
            version: value.version,
            channel: ReleaseChannel::Canary,
        }
    }
}

fn cache_buster() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn latest_json_url() -> String {
    let args: Args = AppEnvironment::cmd_args();
    if let Some(url) = args.use_latest_json_url {
        return url;
    }

    let bucket_url = AppEnvironment::bucket_url();
    format!(
        "{}/{}/latest.json?_t={}",
        bucket_url,
        RELEASE_PREFIX,
        cache_buster()
    )
}

async fn fetch_explorer_latest_release() -> DCLResultTyped<LatestRelease> {
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

fn canary_json_url() -> String {
    let bucket_url = AppEnvironment::bucket_url();
    format!(
        "{}/{}/{}?_t={}",
        bucket_url,
        RELEASE_PREFIX,
        CANARY_JSON_NAME,
        cache_buster()
    )
}

async fn fetch_canary_release() -> DCLResultTyped<CanaryReleaseResponse> {
    let url = canary_json_url();
    log::info!(
        "[fetch_canary_release] Fetching canary release from: {}",
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

    let data = response.json::<CanaryReleaseResponse>().await?;
    log::info!(
        "[fetch_canary_release] Canary release fetched successfully: {:?}",
        data
    );

    Ok(data)
}

async fn get_canary_explorer_release() -> DCLResultTyped<ReleaseResponse> {
    let canary = fetch_canary_release().await?;
    Ok(canary.into())
}

async fn get_latest_explorer_release() -> DCLResultTyped<ReleaseResponse> {
    let url = AppEnvironment::bucket_url();
    let latest_release = fetch_explorer_latest_release().await?;
    let os = get_os_name();
    let release_name = format!("Decentraland_{}.zip", os);
    let release_url = format!(
        "{}/{}/{}/{}",
        url, RELEASE_PREFIX, latest_release.version, release_name
    );

    log::info!(
        "[get_latest_explorer_release] Release URL generated: {{ os: {}, version: {}, url: {} }}",
        os,
        latest_release.version,
        release_url
    );

    let response = ReleaseResponse {
        browser_download_url: release_url,
        version: latest_release.version,
        channel: ReleaseChannel::Latest,
    };

    Ok(response)
}

/// A user is "new" when they have never completed an install: `latest/` is created by the very
/// first successful install, so its absence is the cheapest local signal we have. Existing users
/// are never moved onto the canary track automatically - only the local opt-in flag does that.
fn is_new_user() -> bool {
    !installs::latest_dir_exists()
}

/// Resolves which build this run should download.
pub async fn fetch_explorer_release(
    anon_id: &str,
    prefer_canary_release: bool,
) -> DCLResultTyped<ReleaseResponse> {
    if prefer_canary_release {
        log::info!("[fetch_explorer_release] Canary release requested explicitly");
        return get_canary_explorer_release().await;
    }

    if !is_new_user() {
        log::info!("[fetch_explorer_release] Existing install detected, staying on latest");
        return get_latest_explorer_release().await;
    }

    match fetch_canary_release().await {
        Ok(canary) => {
            let bucket = canary::cohort_bucket(anon_id);
            if canary::is_in_cohort(anon_id, canary.cohort) {
                log::info!(
                    "[fetch_explorer_release] New user in the canary cohort {{ bucket: {}, cohort: {} }}",
                    bucket,
                    canary.cohort
                );
                Ok(canary.into())
            } else {
                log::info!(
                    "[fetch_explorer_release] New user outside the canary cohort {{ bucket: {}, cohort: {} }}",
                    bucket,
                    canary.cohort
                );
                get_latest_explorer_release().await
            }
        }
        Err(e) => {
            log::warn!(
                "[fetch_explorer_release] Cannot resolve the canary release, falling back to latest: {:#}",
                e
            );
            get_latest_explorer_release().await
        }
    }
}
