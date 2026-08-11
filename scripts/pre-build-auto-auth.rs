#!/usr/bin/env rust-script
//! Builds the auto-auth token fetcher and stages it as the
//! `resources/auto-auth-token-fetch` bundle resource Tauri expects.
//!
//! Usage: `rust-script scripts/pre-build-auto-auth.rs`

use std::env;
use std::fs;

const SCRIPT_NAME: &str = "pre-build-auto-auth";
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

include!("shared.rs");
