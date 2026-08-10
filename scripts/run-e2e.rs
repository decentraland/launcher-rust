#!/usr/bin/env rust-script
//! Runs the end-to-end suites (both layers) against a freshly built debug
//! service.
//!
//! Usage: `rust-script scripts/run-e2e.rs` (or `npm run e2e`)

use std::env;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

/// E2E runs are fully local: no external services are contacted.
/// The compile-time bucket URL is a non-routable placeholder (port 9 =
/// discard) — a tripwire: if the runtime DCL_LAUNCHER_BUCKET_URL override
/// ever breaks, requests hit a dead port and tests fail loudly instead of
/// silently reaching production S3. At runtime every test injects its own
/// 127.0.0.1 mock CDN, sandbox base dir, and private IPC endpoint.
const BUCKET_URL_KEY: &str = "VITE_AWS_S3_BUCKET_PUBLIC_URL";
const BUCKET_URL_PLACEHOLDER: &str = "http://127.0.0.1:9/e2e-placeholder";

fn main() {
    let root = repo_root();

    println!("Building the debug service under test...");
    cargo(&root.join("src-service"), &["build"]);

    // The src-tauri build script (run by Layer 2's `cargo test`) refuses to
    // run unless the bundle's externalBin sidecar and, on Windows, the
    // auto-auth resource exist. Both are gitignored — stage them with the
    // pre-build scripts so a fresh clone works.
    if cfg!(windows) {
        rust_script(&root, "scripts/pre-build-auto-auth.rs");
    }
    rust_script(&root, "scripts/pre-build-service.rs");

    println!("Running e2e (Layer 1: service over IPC)...");
    cargo(&root.join("tests-e2e"), &["test", "--", "--include-ignored"]);

    println!("Running e2e (Layer 2: UI-side service lifecycle)...");
    cargo(
        &root.join("src-tauri"),
        &["test", "--tests", "--", "--include-ignored"],
    );

    println!("All e2e tests passed.");
}

/// Delegates to a sibling rust-script (same mechanism as
/// `pre-build-sidecars.rs`): `RUST_SCRIPT` can point at a specific
/// rust-script binary, otherwise it is resolved from `PATH`.
fn rust_script(root: &Path, script: &str) {
    println!("Running {script}...");
    let runner = env::var("RUST_SCRIPT").unwrap_or_else(|_| "rust-script".to_string());
    let status = Command::new(&runner)
        .arg(script)
        .current_dir(root)
        .status()
        .unwrap_or_else(|e| fail(&format!("cannot run {runner}: {e}")));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

/// Runs cargo from INSIDE the crate dir so the crate's own `.cargo/config.toml`
/// (rustflags incl. crt-static) applies — `--manifest-path` from the repo root
/// would pick up the root config and rebuild everything with different flags.
fn cargo(crate_dir: &Path, args: &[&str]) {
    let status = Command::new("cargo")
        .args(args)
        .current_dir(crate_dir)
        .env(BUCKET_URL_KEY, BUCKET_URL_PLACEHOLDER)
        .status()
        .unwrap_or_else(|e| fail(&format!("cannot run cargo in {}: {e}", crate_dir.display())));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

// --- shared script helpers ---------------------------------------------------

/// Walks up from the current directory to the repo root, so the script behaves
/// the same whether it is invoked from the root, from `scripts/`, or by npm.
fn repo_root() -> PathBuf {
    let start = env::current_dir().unwrap_or_else(|e| fail(&format!("cannot read cwd: {e}")));
    for dir in start.ancestors() {
        if dir.join("package.json").is_file() && dir.join("src-tauri").is_dir() {
            return dir.to_path_buf();
        }
    }
    fail(&format!("repo root not found at or above {}", start.display()));
}

fn fail(message: &str) -> ! {
    eprintln!("run-e2e: {message}");
    exit(1);
}
