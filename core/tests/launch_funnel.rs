//! The launch funnel end to end, without the UI: `latest.json`, the zip or the delta set,
//! extract or rebuild, commit, and the attempt to start the Explorer, all against a bucket
//! served from localhost.

mod support;

use std::fs;

use dcl_launcher_core::errors::DCLError;
use dcl_launcher_core::fsm::Installation;
use dcl_launcher_core::version::Version;
use support::{BuildFile, EXPLORER_BIN, TestEnv};

fn read(path: &std::path::Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[tokio::test]
async fn fresh_install_from_zip_then_second_launch_is_up_to_date() {
    let env = TestEnv::new();
    env.bucket
        .publish_zip("v0.1.0", &support::release_zip("v0.1.0"));
    env.bucket.publish_latest("v0.1.0");

    let run = env.run_launch().await;
    assert!(
        run.reached_the_launch(),
        "unexpected end: {:?}",
        run.error()
    );
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
    // The launch attempt fails at once, so the watch may coalesce `launching` into the error.
    assert!(
        run.step_names().starts_with(&[
            "fetching".into(),
            "downloading".into(),
            "installing".into()
        ]),
        "the funnel reports every step once: {:?}",
        run.step_names()
    );
    assert_eq!(run.download_build_type().as_deref(), Some("new"));

    let run = env.run_launch().await;
    assert!(
        run.reached_the_launch(),
        "unexpected end: {:?}",
        run.error()
    );
    assert!(
        !run.step_names()
            .iter()
            .any(|s| s == "downloading" || s == "installing"),
        "an up-to-date install skips download and install: {:?}",
        run.step_names()
    );
}

#[tokio::test]
async fn update_from_zip_keeps_the_previous_version_for_rollback() {
    let env = TestEnv::new();
    env.bucket
        .publish_zip("v0.1.0", &support::release_zip("v0.1.0"));
    env.bucket.publish_latest("v0.1.0");
    assert!(env.run_launch().await.reached_the_launch());

    env.bucket
        .publish_zip("v0.2.0", &support::release_zip("v0.2.0"));
    env.bucket.publish_latest("v0.2.0");
    let run = env.run_launch().await;
    assert!(
        run.reached_the_launch(),
        "unexpected end: {:?}",
        run.error()
    );

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
    assert_eq!(run.download_build_type().as_deref(), Some("update"));
}

#[tokio::test]
async fn missing_zip_fails_with_the_http_code_error() {
    let env = TestEnv::new();
    env.bucket.publish_latest("v0.1.0");

    let run = env.run_launch().await;
    assert!(
        matches!(
            run.error(),
            Some(DCLError::E2004_DOWNLOAD_FAILED_HTTP_CODE { code: 404, .. })
        ),
        "unexpected end: {:?}",
        run.error()
    );
    assert!(
        run.error_status_message().is_some(),
        "the UI gets the error arm"
    );
    assert!(!env.latest_dir().exists());
}

// ---- Delta updates ----------------------------------------------------------------------

/// The installed build: the executable, a data file, a bundle that the update removes and a
/// file the update leaves alone.
fn base_build() -> Vec<BuildFile> {
    let mut files = support::build_files("v0.1.0");
    files.push(BuildFile::new(
        "Decentraland_Data/old.bundle",
        "old bundle",
        0o644,
    ));
    files.push(BuildFile::new("unchanged.txt", "same bytes", 0o644));
    files
}

/// The next build: both carried files change, a bundle is renamed, one file is unchanged.
fn target_build() -> Vec<BuildFile> {
    let mut files = support::build_files("v0.2.0");
    files.push(BuildFile::new(
        "Decentraland_Data/new.bundle",
        "new bundle",
        0o644,
    ));
    files.push(BuildFile::new("unchanged.txt", "same bytes", 0o644));
    files
}

/// A zip of the target build with one extra file, so a test can tell the zip from the delta.
fn target_zip_with_marker() -> Vec<u8> {
    let mut files = target_build();
    files.push(BuildFile::new("from-zip.txt", "zip", 0o644));
    support::build_zip(&files)
}

fn assert_latest_is_the_target_build(env: &TestEnv) {
    for file in target_build() {
        assert_eq!(
            fs::read(env.latest_dir().join(&file.path)).expect("target file"),
            file.content,
            "{} differs",
            file.path
        );
    }
    assert!(
        !env.latest_dir()
            .join("Decentraland_Data/old.bundle")
            .exists(),
        "a deleted path is gone"
    );
    assert_eq!(env.installed_version().as_deref(), Some("v0.2.0"));
}

#[tokio::test]
async fn delta_update_rebuilds_the_new_build_without_the_zip() {
    let Some(hdiffz) = support::hdiffz() else {
        return;
    };
    let env = TestEnv::new();
    env.bucket
        .publish_zip("v0.1.0", &support::build_zip(&base_build()));
    env.bucket.publish_latest("v0.1.0");
    assert!(env.run_launch().await.reached_the_launch());

    let delta =
        support::make_delta_set(&hdiffz, "v0.1.0", &base_build(), "v0.2.0", &target_build());
    delta.publish(&env.bucket, "v0.2.0");
    env.bucket.publish_latest("v0.2.0");
    let run = env.run_launch().await;
    assert!(
        run.reached_the_launch(),
        "unexpected end: {:?}",
        run.error()
    );

    assert_latest_is_the_target_build(&env);
    assert!(
        !env.latest_dir().join("from-zip.txt").exists(),
        "no zip was published, so the build can only come from the patches"
    );
    assert!(
        read(&env.version_dir("v0.1.0").join(EXPLORER_BIN)) == "explorer binary v0.1.0",
        "the previous build is kept for rollback"
    );
    assert!(
        !env.data_dir.join("downloads/delta/v0.2.0").exists(),
        "the patch set is removed after the install"
    );
    assert!(
        run.step_names().starts_with(&[
            "fetching".into(),
            "downloading".into(),
            "installing".into()
        ]),
        "{:?}",
        run.step_names()
    );
    let target = Version::parse("v0.2.0").expect("version");
    assert!(matches!(
        run.state
            .local_installations
            .scanned()
            .and_then(|installations| installations.get(&target)),
        Some(Installation::Ready(_))
    ));
}

/// A bundle whose patch path, `<data>/downloads/delta/<target>/from/<base>/<build id>/<path>.hdiff`,
/// is past Windows' 260-character `MAX_PATH`, like the Explorer's `monoscripts` bundles.
fn deep_bundle_path() -> String {
    format!(
        "Decentraland_Data/StreamingAssets/aa/StandaloneWindows64/{}_monoscripts_{}.bundle",
        "9b420c6596f625d0c10053fb6e48e84e".repeat(2),
        "72c4a5f3c31db1d176a6ef850b07bd28".repeat(2)
    )
}

#[tokio::test]
async fn delta_update_patches_files_past_max_path() {
    let Some(hdiffz) = support::hdiffz() else {
        return;
    };
    let mut base = base_build();
    base.push(BuildFile::new(
        &deep_bundle_path(),
        "deep bundle v0.1.0",
        0o644,
    ));
    let mut target = target_build();
    target.push(BuildFile::new(
        &deep_bundle_path(),
        "deep bundle v0.2.0",
        0o644,
    ));

    let env = TestEnv::new();
    env.bucket.publish_zip("v0.1.0", &support::build_zip(&base));
    env.bucket.publish_latest("v0.1.0");
    assert!(env.run_launch().await.reached_the_launch());

    let delta = support::make_delta_set(&hdiffz, "v0.1.0", &base, "v0.2.0", &target);
    delta.publish(&env.bucket, "v0.2.0");
    env.bucket.publish_latest("v0.2.0");
    let patch_path = env
        .data_dir
        .join("downloads/delta/v0.2.0/from/v0.1.0/deadbeef-e1")
        .join(format!("{}.hdiff", deep_bundle_path()));
    assert!(
        patch_path.as_os_str().len() > 260,
        "the patch path must exceed MAX_PATH to cover the bug: {}",
        patch_path.display()
    );

    // No zip is published, so the launch only succeeds if `hpatchz` opens the deep paths.
    let run = env.run_launch().await;
    assert!(
        run.reached_the_launch(),
        "unexpected end: {:?}",
        run.error()
    );
    assert_latest_is_the_target_build(&env);
    assert_eq!(
        read(&env.latest_dir().join(deep_bundle_path())),
        "deep bundle v0.2.0"
    );
}

#[tokio::test]
async fn missing_manifest_takes_the_zip() {
    let env = TestEnv::new();
    env.bucket
        .publish_zip("v0.1.0", &support::build_zip(&base_build()));
    env.bucket.publish_latest("v0.1.0");
    assert!(env.run_launch().await.reached_the_launch());

    env.bucket.publish_zip("v0.2.0", &target_zip_with_marker());
    env.bucket.publish_latest("v0.2.0");
    let run = env.run_launch().await;
    assert!(
        run.reached_the_launch(),
        "unexpected end: {:?}",
        run.error()
    );
    assert_latest_is_the_target_build(&env);
    assert!(env.latest_dir().join("from-zip.txt").exists());
}

#[tokio::test]
async fn corrupt_patch_falls_back_to_the_zip() {
    let Some(hdiffz) = support::hdiffz() else {
        return;
    };
    let env = TestEnv::new();
    env.bucket
        .publish_zip("v0.1.0", &support::build_zip(&base_build()));
    env.bucket.publish_latest("v0.1.0");
    assert!(env.run_launch().await.reached_the_launch());

    let delta =
        support::make_delta_set(&hdiffz, "v0.1.0", &base_build(), "v0.2.0", &target_build());
    delta.publish(&env.bucket, "v0.2.0");
    env.bucket.publish_zip("v0.2.0", &target_zip_with_marker());
    env.bucket.publish_latest("v0.2.0");
    let first_patch = delta.patch_keys("v0.2.0").remove(0);
    env.bucket
        .with_faults(|faults| faults.corrupt.push(first_patch));

    let run = env.run_launch().await;
    assert!(
        run.reached_the_launch(),
        "unexpected end: {:?}",
        run.error()
    );
    assert_latest_is_the_target_build(&env);
    assert!(
        env.latest_dir().join("from-zip.txt").exists(),
        "the corrupt patch set falls back to the zip within the same launch"
    );
}

#[tokio::test]
async fn tampered_install_falls_back_to_the_zip() {
    let Some(hdiffz) = support::hdiffz() else {
        return;
    };
    let env = TestEnv::new();
    env.bucket
        .publish_zip("v0.1.0", &support::build_zip(&base_build()));
    env.bucket.publish_latest("v0.1.0");
    assert!(env.run_launch().await.reached_the_launch());
    fs::write(
        env.latest_dir().join(support::data_file_path()),
        "modified by something else",
    )
    .expect("tamper");

    let delta =
        support::make_delta_set(&hdiffz, "v0.1.0", &base_build(), "v0.2.0", &target_build());
    delta.publish(&env.bucket, "v0.2.0");
    env.bucket.publish_zip("v0.2.0", &target_zip_with_marker());
    env.bucket.publish_latest("v0.2.0");

    let run = env.run_launch().await;
    assert!(
        run.reached_the_launch(),
        "unexpected end: {:?}",
        run.error()
    );
    assert_latest_is_the_target_build(&env);
    assert!(env.latest_dir().join("from-zip.txt").exists());
}

#[tokio::test]
async fn base_not_listed_takes_the_zip() {
    let Some(hdiffz) = support::hdiffz() else {
        return;
    };
    let env = TestEnv::new();
    env.bucket
        .publish_zip("v0.0.9", &support::build_zip(&base_build()));
    env.bucket.publish_latest("v0.0.9");
    assert!(env.run_launch().await.reached_the_launch());

    let delta =
        support::make_delta_set(&hdiffz, "v0.1.0", &base_build(), "v0.2.0", &target_build());
    delta.publish(&env.bucket, "v0.2.0");
    env.bucket.publish_zip("v0.2.0", &target_zip_with_marker());
    env.bucket.publish_latest("v0.2.0");

    let run = env.run_launch().await;
    assert!(
        run.reached_the_launch(),
        "unexpected end: {:?}",
        run.error()
    );
    assert!(env.latest_dir().join("from-zip.txt").exists());
}
