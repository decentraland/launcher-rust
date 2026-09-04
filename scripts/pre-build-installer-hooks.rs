#!/usr/bin/env rust-script
//! Builds the installer-hooks helper and stages it as the
//! `resources/installer-hooks` bundle resource Tauri expects.
//!
//! Usage: `rust-script scripts/pre-build-installer-hooks.rs`

use std::env;
use std::fs;

const SCRIPT_NAME: &str = "pre-build-installer-hooks";
const INSTALLER_HOOKS_BIN: &str = "installer-hooks";
const INSTALLER_HOOKS_MANIFEST: &str = "installer-hooks/Cargo.toml";
const RESOURCE_NAME: &str = "installer-hooks";

fn main() {
    let root = repo_root();

    run(&root, "cargo", &["build", "--manifest-path", INSTALLER_HOOKS_MANIFEST, "--release"]);

    let resources_dir = root.join("src-tauri").join("resources");
    fs::create_dir_all(&resources_dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", resources_dir.display())));

    let built = root
        .join("installer-hooks/target/release")
        .join(format!("{INSTALLER_HOOKS_BIN}{}", env::consts::EXE_SUFFIX));
    let staged = resources_dir.join(format!("{RESOURCE_NAME}{}", env::consts::EXE_SUFFIX));
    copy(&built, &staged);

    println!("InstallerHooks resource ready: {}", staged.display());
}

include!("shared.rs");
