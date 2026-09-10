#!/usr/bin/env rust-script
//! Builds the `dcl_watchdog` sidecar and stages it in `src-tauri/binaries/`
//! under the `-<target-triple>` name Tauri expects for `externalBin`.
//!
//! Usage: `rust-script scripts/pre-build-watchdog.rs`
//!
//! The target triple comes from `TAURI_ENV_TARGET_TRIPLE` when Tauri sets it,
//! otherwise from the host. `universal-apple-darwin` builds both macOS arches,
//! `lipo`s them together and also stages the fat binary under both per-arch
//! names: the bundler looks up `-universal-apple-darwin`, while `tauri-build`
//! (bare `cargo` in src-tauri, and each arch of a universal build) looks up the
//! arch it is compiling for.

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

const SCRIPT_NAME: &str = "pre-build-watchdog";
const WATCHDOG_BIN: &str = "dcl_watchdog";
const WATCHDOG_MANIFEST: &str = "client-crash-watchdog/Cargo.toml";
const UNIVERSAL_DARWIN: &str = "universal-apple-darwin";
const DARWIN_ARCHES: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

fn main() {
    let root = repo_root();
    let triple = target_triple();

    let binaries_dir = root.join("src-tauri").join("binaries");
    fs::create_dir_all(&binaries_dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", binaries_dir.display())));

    let staged = binaries_dir.join(staged_name(&triple));

    if triple == UNIVERSAL_DARWIN {
        build_universal_darwin(&root, &staged);
        for arch in DARWIN_ARCHES {
            copy(&staged, &binaries_dir.join(staged_name(arch)));
        }
    } else {
        build_native(&root, &staged);
    }

    println!("Watchdog sidecar ready: {}", staged.display());
}

fn staged_name(triple: &str) -> String {
    format!("{WATCHDOG_BIN}-{triple}{}", env::consts::EXE_SUFFIX)
}

/// Single-arch build: cargo is driven by `--manifest-path` from the repo root
/// so the root `.cargo/config.toml` stays in effect (lint denials, crt-static).
fn build_native(root: &Path, staged: &Path) {
    run(root, "cargo", &["build", "--manifest-path", WATCHDOG_MANIFEST, "--release"]);

    let built = root
        .join("client-crash-watchdog/target/release")
        .join(format!("{WATCHDOG_BIN}{}", env::consts::EXE_SUFFIX));
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
            &["build", "--manifest-path", WATCHDOG_MANIFEST, "--release", "--target", arch],
        );
    }

    let mut lipo = vec!["-create".to_string()];
    for arch in DARWIN_ARCHES {
        lipo.push(format!("client-crash-watchdog/target/{arch}/release/{WATCHDOG_BIN}"));
    }
    lipo.push("-output".to_string());
    lipo.push(staged.display().to_string());
    run(root, "lipo", &lipo);
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

include!("shared.rs");
