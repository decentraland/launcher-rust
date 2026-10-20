#!/usr/bin/env rust-script
//! Stages the `hpatchz` sidecar (HDiffPatch, MIT) in `src-tauri/binaries/`
//! under the `-<target-triple>` name Tauri expects for `externalBin`.
//!
//! Usage: `rust-script scripts/pre-build-hpatchz.rs`
//!
//! The pinned release zip is downloaded with `curl`, checked against its
//! sha256 and unpacked with `tar`. The version, the asset names and the
//! digests mirror `scripts/delta/src/shell/hdiff.rs` in unity-explorer: the
//! patches are produced with that build, so the launcher applies them with it.
//!
//! ```cargo
//! [dependencies]
//! sha2 = "0.10"
//! hex = "0.4"
//! ```

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use sha2::Digest;

const SCRIPT_NAME: &str = "pre-build-hpatchz";
const HPATCHZ_BIN: &str = "hpatchz";
const VERSION: &str = "v5.1.3";
const UNIVERSAL_DARWIN: &str = "universal-apple-darwin";
const DARWIN_ARCHES: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

/// Release asset and the sha256 of its zip, per OS.
fn pinned() -> (&'static str, &'static str) {
    match env::consts::OS {
        "windows" => (
            "windows64",
            "77f141386e5d8f785c1c846e10fbbc19b6c05aa00e3f59cc44670fb3f0e2ae94",
        ),
        "macos" => (
            "macos",
            "9e9d7318db1ea5607dbdde41e6614cfe5b73c589034b38ee91a79f4ca80a940c",
        ),
        "linux" => (
            "linux64",
            "628963bf2ee9108a97260fa5eef44acd9ec94369b76090a957c9182b3abbb558",
        ),
        os => fail(&format!("no pinned HDiffPatch build for {os}")),
    }
}

fn main() {
    let root = repo_root();
    let triple = target_triple();
    let (asset, sha256) = pinned();

    let binaries_dir = root.join("src-tauri").join("binaries");
    fs::create_dir_all(&binaries_dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", binaries_dir.display())));

    let tools_dir = root.join("src-tauri").join("target").join("hdiffpatch");
    fs::create_dir_all(&tools_dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", tools_dir.display())));

    let archive = tools_dir.join(format!("hdiffpatch_{VERSION}_bin_{asset}.zip"));
    if !archive.is_file() || sha256_of(&archive) != sha256 {
        let url = format!(
            "https://github.com/sisong/HDiffPatch/releases/download/{VERSION}/hdiffpatch_{VERSION}_bin_{asset}.zip"
        );
        println!("Downloading {url}");
        run(
            &root,
            "curl",
            &["-fsSL", "--retry", "3", "-o", &archive.display().to_string(), &url],
        );
    }
    let actual = sha256_of(&archive);
    if actual != sha256 {
        let _ = fs::remove_file(&archive);
        fail(&format!("HDiffPatch sha256 mismatch: got {actual}, want {sha256}"));
    }

    run(
        &tools_dir,
        "tar",
        &["-xf", &archive.display().to_string()],
    );
    let built = tools_dir
        .join(asset)
        .join(format!("{HPATCHZ_BIN}{}", env::consts::EXE_SUFFIX));
    if !built.is_file() {
        fail(&format!("{} is missing after unpacking", built.display()));
    }

    let staged = binaries_dir.join(staged_name(&triple));
    copy(&built, &staged);
    if triple == UNIVERSAL_DARWIN {
        for arch in DARWIN_ARCHES {
            copy(&staged, &binaries_dir.join(staged_name(arch)));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| fail(&format!("cannot chmod {}: {e}", staged.display())));
    }
    println!("hpatchz sidecar ready: {}", staged.display());
}

fn staged_name(triple: &str) -> String {
    format!("{HPATCHZ_BIN}-{triple}{}", env::consts::EXE_SUFFIX)
}

fn sha256_of(path: &Path) -> String {
    match fs::read(path) {
        Ok(bytes) => hex::encode(sha2::Sha256::digest(bytes)),
        Err(_) => String::new(),
    }
}

/// `TAURI_ENV_TARGET_TRIPLE` when Tauri sets it, otherwise the host triple from `rustc -vV`.
fn target_triple() -> String {
    match env::var("TAURI_ENV_TARGET_TRIPLE") {
        Ok(triple) if !triple.is_empty() => triple,
        _ => host_triple(),
    }
}

fn host_triple() -> String {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .unwrap_or_else(|e| fail(&format!("cannot run rustc: {e}")));
    if !output.status.success() {
        fail("rustc -vV failed");
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
        .unwrap_or_else(|| fail("no `host:` line in rustc -vV output"))
}

include!("shared.rs");
