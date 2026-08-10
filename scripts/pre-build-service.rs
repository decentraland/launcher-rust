#!/usr/bin/env rust-script
//! Builds the `dcl_launcher_service` sidecar and stages it in
//! `src-tauri/binaries/` under the `-<target-triple>` name Tauri expects for
//! `externalBin`.
//!
//! Usage: `rust-script scripts/pre-build-service.rs`
//!
//! The target triple comes from `TAURI_ENV_TARGET_TRIPLE` when Tauri sets it,
//! otherwise from the host. `universal-apple-darwin` builds both macOS arches
//! and `lipo`s them together.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const SERVICE_BIN: &str = "dcl_launcher_service";
const SERVICE_MANIFEST: &str = "src-service/Cargo.toml";
const UNIVERSAL_DARWIN: &str = "universal-apple-darwin";
const DARWIN_ARCHES: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

fn main() {
    let root = repo_root();
    let triple = target_triple();

    let binaries_dir = root.join("src-tauri").join("binaries");
    fs::create_dir_all(&binaries_dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", binaries_dir.display())));

    let staged = binaries_dir.join(format!("{SERVICE_BIN}-{triple}{}", env::consts::EXE_SUFFIX));

    if triple == UNIVERSAL_DARWIN {
        build_universal_darwin(&root, &staged);
    } else {
        build_native(&root, &staged);
    }

    println!("Service sidecar ready: {}", staged.display());
}

/// Single-arch build: cargo is driven by `--manifest-path` from the repo root
/// so the root `.cargo/config.toml` stays in effect, exactly as the shell
/// scripts this replaced did.
fn build_native(root: &Path, staged: &Path) {
    run(root, "cargo", &["build", "--manifest-path", SERVICE_MANIFEST, "--release"]);

    let built = root
        .join("src-service/target/release")
        .join(format!("{SERVICE_BIN}{}", env::consts::EXE_SUFFIX));
    copy(&built, staged);
}

fn build_universal_darwin(root: &Path, staged: &Path) {
    let mut add_targets = vec!["target", "add"];
    add_targets.extend(DARWIN_ARCHES);
    run(root, "rustup", &add_targets);

    for arch in DARWIN_ARCHES {
        run(
            root,
            "cargo",
            &["build", "--manifest-path", SERVICE_MANIFEST, "--release", "--target", arch],
        );
    }

    let mut lipo = vec!["-create".to_string()];
    for arch in DARWIN_ARCHES {
        lipo.push(format!("src-service/target/{arch}/release/{SERVICE_BIN}"));
    }
    lipo.push("-output".to_string());
    lipo.push(staged.display().to_string());
    run(root, "lipo", &lipo);
}

fn copy(from: &Path, to: &Path) {
    fs::copy(from, to).unwrap_or_else(|e| {
        fail(&format!("cannot copy {} to {}: {e}", from.display(), to.display()))
    });
}

// --- shared script helpers ---------------------------------------------------

/// Walks up from the current directory to the repo root, so the script behaves
/// the same whether it is invoked from the root, from `scripts/`, or by Tauri.
fn repo_root() -> PathBuf {
    let start = env::current_dir().unwrap_or_else(|e| fail(&format!("cannot read cwd: {e}")));
    for dir in start.ancestors() {
        if dir.join("package.json").is_file() && dir.join("src-tauri").is_dir() {
            return dir.to_path_buf();
        }
    }
    fail(&format!("repo root not found at or above {}", start.display()));
}

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

fn run<S: AsRef<OsStr>>(cwd: &Path, program: &str, args: &[S]) {
    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap_or_else(|e| fail(&format!("cannot run {program}: {e}")));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

fn fail(message: &str) -> ! {
    eprintln!("pre-build-service: {message}");
    exit(1);
}
