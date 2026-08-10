#!/usr/bin/env rust-script
//! Builds every sidecar/resource binary the Windows bundle needs, in order.
//!
//! Usage: `rust-script scripts/pre-build-sidecars.rs`
//!
//! Each step is delegated to its own script so the individual steps stay
//! usable on their own (CI runs them separately).

use std::env;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const STEPS: [&str; 2] = ["scripts/pre-build-auto-auth.rs", "scripts/pre-build-service.rs"];

/// Set `RUST_SCRIPT` to point at a specific rust-script binary; otherwise it is
/// resolved from `PATH`.
const RUST_SCRIPT_ENV: &str = "RUST_SCRIPT";

fn main() {
    let root = repo_root();
    let runner = env::var(RUST_SCRIPT_ENV).unwrap_or_else(|_| "rust-script".to_string());

    for step in STEPS {
        println!("Running {step}...");
        run(&root, &runner, &[step]);
    }
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

fn run(cwd: &Path, program: &str, args: &[&str]) {
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
    eprintln!("pre-build-sidecars: {message}");
    exit(1);
}
