use anyhow::{Context, Result, anyhow};
use reqwest;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

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

/// `<bucket>/<RELEASE_PREFIX>/<segments...>` on the configured bucket.
pub fn release_file_url(segments: &[&str]) -> Result<Url> {
    join_release_path(AppEnvironment::bucket_url()?, segments)
}

/// Appends the release prefix and `segments` as path segments, so the bucket URL may end with
/// or without a slash and no segment is ever interpreted as a query or a fragment.
fn join_release_path(mut bucket: Url, segments: &[&str]) -> Result<Url> {
    let base = bucket.to_string();
    bucket
        .path_segments_mut()
        .map_err(|()| anyhow!("bucket URL cannot hold a path: {base}"))?
        .pop_if_empty()
        .extend(RELEASE_PREFIX.split('/'))
        .extend(segments);
    Ok(bucket)
}

fn latest_json_url() -> Result<Url> {
    let args: Args = AppEnvironment::cmd_args();
    if let Some(raw) = args.use_latest_json_url {
        return Url::parse(&raw).with_context(|| format!("--use-latest-json-url {raw:?}"));
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut url = release_file_url(&["latest.json"])?;
    url.query_pairs_mut()
        .append_pair("_t", &timestamp.to_string());
    Ok(url)
}

async fn fetch_explorer_latest_release() -> DCLErrorTyped<LatestRelease> {
    let url = latest_json_url()?;
    log::info!(
        "[fetch_explorer_latest_release] Fetching latest release from: {}",
        url
    );

    let response = reqwest::get(url.clone()).await?;

    if !response.status().is_success() {
        return DCLError::E2004_DOWNLOAD_FAILED_HTTP_CODE {
            url: url.to_string(),
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

pub async fn get_latest_explorer_release() -> DCLErrorTyped<ReleaseResponse> {
    let latest_release = fetch_explorer_latest_release().await?;
    let os = get_os_name();
    let release_name = format!("Decentraland_{}.zip", os);
    let release_url = release_file_url(&[latest_release.version.as_str(), &release_name])?;

    log::info!(
        "[get_latest_explorer_release] Release URL generated: {{ os: {}, version: {}, url: {} }}",
        os,
        latest_release.version,
        release_url
    );

    let response = ReleaseResponse {
        browser_download_url: release_url.to_string(),
        version: latest_release.version,
    };

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(bucket: &str, segments: &[&str]) -> Result<Url> {
        join_release_path(Url::parse(bucket)?, segments)
    }

    #[test]
    fn join_release_path_ignores_a_trailing_slash_on_the_bucket() -> Result<()> {
        let expected = "http://127.0.0.1:9/@dcl/unity-explorer/releases/v1.2.3/Decentraland_windows64.zip";
        let segments = ["v1.2.3", "Decentraland_windows64.zip"];
        assert_eq!(joined("http://127.0.0.1:9", &segments)?.as_str(), expected);
        assert_eq!(joined("http://127.0.0.1:9/", &segments)?.as_str(), expected);
        Ok(())
    }

    #[test]
    fn join_release_path_keeps_a_bucket_prefix() -> Result<()> {
        let expected = "https://cdn.example.com/bucket/@dcl/unity-explorer/releases/latest.json";
        assert_eq!(
            joined("https://cdn.example.com/bucket/", &["latest.json"])?.as_str(),
            expected
        );
        assert_eq!(
            joined("https://cdn.example.com/bucket", &["latest.json"])?.as_str(),
            expected
        );
        Ok(())
    }

    #[test]
    fn join_release_path_escapes_segments_instead_of_splitting_them() -> Result<()> {
        let joined = joined("http://h", &["a/b?c#d"])?;
        assert_eq!(joined.path(), "/@dcl/unity-explorer/releases/a%2Fb%3Fc%23d");
        assert!(joined.query().is_none());
        assert!(joined.fragment().is_none());
        Ok(())
    }
}
