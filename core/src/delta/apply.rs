//! Building `<app>/<target>/` from the installed build and a downloaded patch set, verified
//! file by file against the manifest before the regular install commit takes it.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::errors::{DCLError, DCLErrorResult};
use crate::installs;
use crate::version::Version;

use super::download::{empty_file, patches_dir};
use super::manifest::{BaseDelta, BuildPath, FileEntry, Manifest, Sha256Digest};
use super::patcher::Patcher;
use super::sha256_file;

/// How one file of the new build is produced.
enum Source<'a> {
    Patch {
        from: &'a BuildPath,
        patch: &'a BuildPath,
    },
    Whole {
        patch: &'a BuildPath,
    },
    Unchanged,
}

/// Writes every entry of the manifest into `<app>/<target>/`, reading unchanged files and
/// patch inputs from the untouched installed build. Removes the patch set afterwards.
pub fn build_version_from_delta(
    target: &Version,
    manifest: &Manifest,
    delta: &BaseDelta,
    patcher: &Patcher,
) -> DCLErrorResult {
    let installed = installs::explorer_latest_version_path();
    let out = installs::explorer_version_dir(&target.to_string());
    if out.exists() {
        fs::remove_dir_all(&out).map_err(|source| DCLError::E3005_STALE_BUILD_CLEANUP_FAILED {
            path: out.to_string_lossy().into_owned(),
            source,
        })?;
    }
    fs::create_dir_all(&out)?;

    let patch_set = patches_dir(target);
    let empty = empty_file(target);
    let sources = sources(delta);
    for entry in &manifest.files {
        match entry {
            FileEntry::Link { path, symlink } => {
                make_link(&out.join(path.as_str()), symlink.as_str())?;
            }
            FileEntry::File {
                path, sha256, mode, ..
            } => {
                let destination = out.join(path.as_str());
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)?;
                }
                match sources.get(path).unwrap_or(&Source::Unchanged) {
                    Source::Patch { from, patch } => apply_patch(
                        patcher,
                        &installed.join(from.as_str()),
                        &patch_set.join(patch.as_str()),
                        &destination,
                        path,
                    )?,
                    Source::Whole { patch } => apply_patch(
                        patcher,
                        &empty,
                        &patch_set.join(patch.as_str()),
                        &destination,
                        path,
                    )?,
                    Source::Unchanged => {
                        copy_unchanged(&installed.join(path.as_str()), &destination, path, sha256)?;
                    }
                }
                verify(&destination, path, sha256)?;
                set_mode(&destination, mode.bits())?;
            }
        }
    }
    let _ = fs::remove_dir_all(&patch_set);
    Ok(())
}

fn sources(delta: &BaseDelta) -> BTreeMap<&BuildPath, Source<'_>> {
    let mut sources = BTreeMap::new();
    for patch in &delta.patches {
        sources.insert(
            &patch.path,
            Source::Patch {
                from: &patch.from_path,
                patch: &patch.patch,
            },
        );
    }
    for whole in &delta.whole {
        sources.insert(
            &whole.path,
            Source::Whole {
                patch: &whole.patch,
            },
        );
    }
    sources
}

fn apply_patch(
    patcher: &Patcher,
    old: &Path,
    patch_file: &Path,
    out: &Path,
    built: &BuildPath,
) -> DCLErrorResult {
    patcher
        .apply(old, patch_file, out)
        .map_err(|source| DCLError::E4007_DELTA_PATCH_APPLY_FAILED {
            path: built.to_string(),
            reason: format!("{source:#}"),
        })
}

/// A file the patch set leaves alone must still be the file the manifest expects.
fn copy_unchanged(
    installed: &Path,
    destination: &Path,
    path: &BuildPath,
    expected: &Sha256Digest,
) -> DCLErrorResult {
    let differs = |reason: String| DCLError::E4005_DELTA_INSTALLED_BUILD_DIFFERS {
        path: path.to_string(),
        reason,
    };
    let actual = sha256_file(installed).map_err(|e| differs(e.to_string()))?;
    if actual != *expected {
        return Err(differs(
            "sha256 differs from the manifest's files entry".to_owned(),
        ));
    }
    fs::copy(installed, destination)?;
    Ok(())
}

fn verify(destination: &Path, path: &BuildPath, expected: &Sha256Digest) -> DCLErrorResult {
    let actual = sha256_file(destination)?;
    if actual != *expected {
        return Err(DCLError::E4008_DELTA_BUILD_FAILED_VERIFICATION {
            path: path.to_string(),
        });
    }
    Ok(())
}

#[cfg(unix)]
fn make_link(link: &Path, target: &str) -> DCLErrorResult {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)?;
    }
    if link.symlink_metadata().is_ok() {
        fs::remove_file(link)?;
    }
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

/// Windows builds carry no links; a manifest that lists one is refused.
#[cfg(not(unix))]
fn make_link(link: &Path, target: &str) -> DCLErrorResult {
    Err(DCLError::E4007_DELTA_PATCH_APPLY_FAILED {
        path: link.to_string_lossy().into_owned(),
        reason: format!("symlink to {target} is not supported on this platform"),
    })
}

#[cfg(unix)]
fn set_mode(path: &Path, bits: u32) -> DCLErrorResult {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(bits))?;
    Ok(())
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
const fn set_mode(_path: &Path, _bits: u32) -> DCLErrorResult {
    Ok(())
}
