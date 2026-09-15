use reqwest;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::environment::{AppEnvironment, Args};
use crate::errors::{DCLError, DCLResultTyped};
use crate::installs;
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

#[derive(Deserialize, Debug)]
pub struct CanaryReleaseResponse {
    pub browser_download_url: String,
    pub version: String,
    pub cohort: u8, // range 0..100 - 100 is max and means ALL users under canary
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

async fn get_canary_explorer_release() -> DCLResultTyped<ReleaseResponse> {
    // TODO like get_latest_explorer_release does
    todo!()
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
    };

    Ok(response)
}

async fn is_new_user_under_the_canary_group(_anon_id: &str) -> bool {
    let is_user_considered_new = !installs::latest_dir_exists(); // an existing user already has
                                                                 // latest directory
   
    if !is_user_considered_new {
        return false;
    }

    // fetch the canary release info
    // DO anon_id modulo 100
    // cohort the data and if anon_modulo < cohort then true

    // Do anon_id cohort split
    //
    /*
#[derive(Deserialize, Debug)]
pub struct CanaryReleaseResponse {
    pub browser_download_url: String,
    pub version: String,
    pub cohort: u8, // range 0..100 - 100 is max and means ALL users under canary
}
    */
    todo!();

    // TODO do a fetch by considering the user new and being
    // under the UUID group
    // // TODO do a fetch by considering the user new and being
                                      // under the UUID group
}

pub async fn fetch_explorer_release(anon_id: String, prefer_canary_release: bool) -> DCLResultTyped<ReleaseResponse> {
    let canary_by_preference = prefer_canary_release;
    let canary_by_grouping = is_new_user_under_the_canary_group(&anon_id); 

    if canary_by_preference || canary_by_grouping.await {
        get_canary_explorer_release().await
    } 
    else {
        get_latest_explorer_release().await
    }
}
