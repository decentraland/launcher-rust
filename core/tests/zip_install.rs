//! The zip install path end to end, without the UI: fetch `latest.json`, download the zip,
//! extract and commit it, all against a bucket served from localhost.

mod support;

use dcl_launcher_core::errors::DCLError;
use std::fs;
use support::{CollectingChannel, EXPLORER_BIN, TestEnv};

fn read(path: &std::path::Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[tokio::test]
async fn fresh_install_from_zip_then_second_launch_is_up_to_date() {
    let env = TestEnv::new();
    env.bucket
        .publish_zip("v0.1.0", &support::release_zip("v0.1.0"));
    env.bucket.publish_latest("v0.1.0");

    let (flow, state) = env.new_flow();
    let channel = CollectingChannel::default();
    let handled = flow.prepare(&channel, state).await.expect("first prepare");
    assert!(!handled, "no deeplink, nothing to pass through");

    assert_eq!(
        read(&env.latest_dir().join(EXPLORER_BIN)),
        "explorer binary v0.1.0"
    );
    assert_eq!(env.installed_version().as_deref(), Some("v0.1.0"));
    assert!(
        !env.version_dir("v0.1.0").exists(),
        "the version folder is renamed to latest/"
    );
    assert!(
        !env.data_dir.join("downloads/decentraland.zip").exists(),
        "the consumed download is removed"
    );
    assert_eq!(
        channel.step_names(),
        ["fetching", "downloading", "installing"],
        "the funnel reports every step once"
    );
    assert_eq!(
        channel.download_build_type().as_deref(),
        Some("new"),
        "a fresh install downloads as New"
    );

    let (flow, state) = env.new_flow();
    let channel = CollectingChannel::default();
    flow.prepare(&channel, state).await.expect("second prepare");
    assert_eq!(
        channel.step_names(),
        ["fetching"],
        "an up-to-date install skips download and install"
    );
}

#[tokio::test]
async fn update_from_zip_keeps_the_previous_version_for_rollback() {
    let env = TestEnv::new();
    env.bucket
        .publish_zip("v0.1.0", &support::release_zip("v0.1.0"));
    env.bucket.publish_latest("v0.1.0");
    let (flow, state) = env.new_flow();
    flow.prepare(&CollectingChannel::default(), state)
        .await
        .expect("install v0.1.0");

    env.bucket
        .publish_zip("v0.2.0", &support::release_zip("v0.2.0"));
    env.bucket.publish_latest("v0.2.0");
    let (flow, state) = env.new_flow();
    let channel = CollectingChannel::default();
    flow.prepare(&channel, state)
        .await
        .expect("update to v0.2.0");

    assert_eq!(
        read(&env.latest_dir().join(EXPLORER_BIN)),
        "explorer binary v0.2.0"
    );
    assert_eq!(
        read(&env.latest_dir().join(support::data_file_path())),
        "data v0.2.0"
    );
    assert_eq!(env.installed_version().as_deref(), Some("v0.2.0"));
    assert_eq!(
        read(&env.version_dir("v0.1.0").join(EXPLORER_BIN)),
        "explorer binary v0.1.0",
        "the previous build is renamed back to its version folder"
    );
    assert!(!env.version_dir("v0.2.0").exists());
    assert_eq!(
        channel.step_names(),
        ["fetching", "downloading", "installing"]
    );
    assert_eq!(channel.download_build_type().as_deref(), Some("update"));
}

#[tokio::test]
async fn missing_zip_fails_with_the_http_code_error() {
    let env = TestEnv::new();
    env.bucket.publish_latest("v0.1.0");

    let (flow, state) = env.new_flow();
    let error = flow
        .prepare(&CollectingChannel::default(), state)
        .await
        .expect_err("no zip on the bucket");

    assert!(
        matches!(
            error,
            DCLError::E2004_DOWNLOAD_FAILED_HTTP_CODE { code: 404, .. }
        ),
        "unexpected error: {error:?}"
    );
    assert!(!env.latest_dir().exists());
}
