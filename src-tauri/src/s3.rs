use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};
use reqwest;

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


pub fn bucket_url() -> Result<String> {
    std::env::var("VITE_AWS_S3_BUCKET_PUBLIC_URL")
        .context("Failed to get VITE_AWS_S3_BUCKET_PUBLIC_URL environment variable")
}

async fn fetch_explorer_latest_release() -> Result<LatestRelease> {
    let bucket_url = bucket_url()?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let url = format!(
        "{}/{}/latest.json?_t={}",
        bucket_url, RELEASE_PREFIX, timestamp
    );

    println!(
        "[fetch_explorer_latest_release] Fetching latest release from: {}",
        url
    );

    let response = reqwest::get(&url).await?;

    if !response.status().is_success() {
        return Err(anyhow!("HTTP error with status code: {}", response.status()).into());
    }


    let data = response.json::<LatestRelease>().await?;
    println!(
        "[fetch_explorer_latest_release] Latest release fetched successfully: {:?}",
        data
    );

    Ok(data)
}

pub async fn get_latest_explorer_release() -> Result<ReleaseResponse> {
    let url = bucket_url()?;
    let latest_release = fetch_explorer_latest_release().await?;
    let os = get_os_name();
    let release_name = format!("Decentraland_{}.zip", os);
    let release_url = format!(
        "{}/{}/{}/{}",
        url, RELEASE_PREFIX, latest_release.version, release_name
    );

    println!(
        "[get_latest_explorer_release] Release URL generated: {{ os: {}, version: {}, url: {} }}",
        os, latest_release.version, release_url
    );

    let response = ReleaseResponse {
        browser_download_url: release_url,
        version: latest_release.version,
    };

    Ok(response)
}
