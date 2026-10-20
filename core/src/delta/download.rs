//! Downloading a base's patch set: the installed build is checked first, then the free space,
//! then every patch lands under `downloads/delta/<target>/` with its sha256 verified.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::time::timeout;

use crate::errors::{DCLError, DCLErrorResult};
use crate::installs;
use crate::version::Version;

use super::fetch::patch_url;
use super::manifest::{BaseDelta, FileEntry, Manifest, Sha256Digest};
use super::patcher::Patcher;
use super::sha256_file;

const CHUNK_TIMEOUT: Duration = Duration::from_secs(15);

/// `<app>/downloads/delta/<target>`, where the patches of one update are kept until applied.
pub fn patches_dir(target: &Version) -> PathBuf {
    installs::delta_downloads_path().join(target.to_string())
}

/// The empty file whole-file patches are applied to.
pub fn empty_file(target: &Version) -> PathBuf {
    patches_dir(target).join(".empty")
}

/// Checks the patcher, the installed build and the free space, then downloads the patch set.
/// `progress` receives the percentage of patch bytes landed.
pub async fn download_patches(
    target: &Version,
    manifest: &Manifest,
    delta: &BaseDelta,
    progress: impl Fn(u8),
) -> DCLErrorResult {
    Patcher::locate()?;
    let installed = installs::explorer_latest_version_path();
    check_installed_build(&installed, delta)?;
    check_free_space(&installs::delta_downloads_path(), manifest, delta)?;

    let dir = patches_dir(target);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    fs::write(empty_file(target), b"")?;

    let total = delta.download_bytes().max(1);
    let mut landed: u64 = 0;
    let downloads = delta
        .patches
        .iter()
        .map(|p| (&p.patch, p.patch_size, &p.patch_sha256))
        .chain(
            delta
                .whole
                .iter()
                .map(|w| (&w.patch, w.patch_size, &w.patch_sha256)),
        );
    for (patch, size, sha256) in downloads {
        let destination = dir.join(patch.as_str());
        download_patch(
            &patch_url(target, patch),
            &destination,
            sha256,
            patch.as_str(),
        )
        .await?;
        landed = landed.saturating_add(size);
        progress(percent(landed, total));
    }
    Ok(())
}

/// Every patch starts from a file the installed build must hold unchanged.
fn check_installed_build(installed: &Path, delta: &BaseDelta) -> DCLErrorResult {
    for patch in &delta.patches {
        let path = installed.join(patch.from_path.as_str());
        let actual =
            sha256_file(&path).map_err(|source| DCLError::E4005_DELTA_INSTALLED_BUILD_DIFFERS {
                path: patch.from_path.to_string(),
                reason: source.to_string(),
            })?;
        if actual != patch.from_sha256 {
            return Err(DCLError::E4005_DELTA_INSTALLED_BUILD_DIFFERS {
                path: patch.from_path.to_string(),
                reason: "sha256 differs from the manifest's from_sha256".to_owned(),
            });
        }
    }
    Ok(())
}

/// The patches plus the whole new build must fit on the volume of the app directory.
fn check_free_space(volume_path: &Path, manifest: &Manifest, delta: &BaseDelta) -> DCLErrorResult {
    let build_bytes = manifest
        .files
        .iter()
        .map(|entry| match entry {
            FileEntry::File { size, .. } => *size,
            FileEntry::Link { .. } => 0,
        })
        .fold(0u64, u64::saturating_add);
    let needed = delta.download_bytes().saturating_add(build_bytes);
    let Some(available) = available_space(volume_path) else {
        log::warn!("[delta] cannot read the free space; proceeding");
        return Ok(());
    };
    if available < needed {
        return Err(DCLError::E4009_DELTA_INSUFFICIENT_SPACE { needed, available });
    }
    Ok(())
}

/// Free bytes on the disk holding `path`: the mounted disk with the longest matching prefix.
fn available_space(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|disk| path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(sysinfo::Disk::available_space)
}

async fn download_patch(
    url: &str,
    destination: &Path,
    expected: &Sha256Digest,
    name: &str,
) -> DCLErrorResult {
    let failed = |reason: String| DCLError::E4006_DELTA_PATCH_DOWNLOAD_FAILED {
        patch: name.to_owned(),
        reason,
    };
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let response = reqwest::get(url).await.map_err(|e| failed(e.to_string()))?;
    if !response.status().is_success() {
        return Err(failed(format!("HTTP {}", response.status())));
    }
    let mut file = File::create(destination)?;
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    loop {
        match timeout(CHUNK_TIMEOUT, stream.next()).await {
            Ok(Some(chunk)) => {
                let chunk = chunk.map_err(|e| failed(e.to_string()))?;
                hasher.update(&chunk);
                file.write_all(&chunk)?;
            }
            Ok(None) => break,
            Err(_) => return Err(failed("network timeout".to_owned())),
        }
    }
    file.sync_all()?;
    let digest: [u8; 32] = hasher.finalize().into();
    let actual = Sha256Digest::from(digest);
    if actual != *expected {
        return Err(failed(
            "sha256 differs from the manifest's patch_sha256".to_owned(),
        ));
    }
    Ok(())
}

#[allow(clippy::cast_possible_truncation)]
fn percent(landed: u64, total: u64) -> u8 {
    landed
        .saturating_mul(100)
        .checked_div(total)
        .unwrap_or(0)
        .min(100) as u8
}
