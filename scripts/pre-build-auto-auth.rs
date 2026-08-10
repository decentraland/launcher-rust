#!/usr/bin/env rust-script
//! Builds the auto-auth token fetcher and stages it as the
//! `resources/auto-auth-token-fetch` bundle resource Tauri expects.
//!
//! Usage: `rust-script scripts/pre-build-auto-auth.rs`

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const AUTO_AUTH_BIN: &str = "src-auto-auth";
const AUTO_AUTH_MANIFEST: &str = "src-auto-auth/Cargo.toml";
const RESOURCE_NAME: &str = "auto-auth-token-fetch";

fn main() {
    let root = repo_root();

    run(&root, "cargo", &["build", "--manifest-path", AUTO_AUTH_MANIFEST, "--release"]);

    let resources_dir = root.join("src-tauri").join("resources");
    fs::create_dir_all(&resources_dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", resources_dir.display())));

    let built = root
        .join("src-auto-auth/target/release")
        .join(format!("{AUTO_AUTH_BIN}{}", env::consts::EXE_SUFFIX));
    let staged = resources_dir.join(format!("{RESOURCE_NAME}{}", env::consts::EXE_SUFFIX));
    copy(&built, &staged);

    println!("AutoAuth resource ready: {}", staged.display());
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
    eprintln!("pre-build-auto-auth: {message}");
    exit(1);
}
