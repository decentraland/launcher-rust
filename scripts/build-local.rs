#!/usr/bin/env rust-script
//! Builds the full release bundle locally.
//!
//! Usage: `rust-script scripts/build-local.rs` (or `npm run build-local`)
//!
//! `createUpdaterArtifacts` is on, so `tauri build` refuses to finish without a
//! minisign private key — and the real one only exists as a CI secret. This
//! mints a throwaway keypair for the run, signs with it, and deletes it again.
//! The build is therefore NOT release-signable: its artifacts cannot be served
//! as an update to anyone running a production install.
//!
//! The generated public key is injected into the bundle too, so the local build
//! trusts exactly the key that signed it. Without that it would keep the
//! production pubkey and endpoint, and could auto-update itself to the
//! production release on first launch — silently replacing the build under
//! test. `src-tauri/tauri.conf.json` is never modified on disk.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command, ExitStatus};

const SCRIPT_NAME: &str = "build-local";

/// `src-shared/src/environment.rs` reads this with `env!`, so nothing compiles
/// without it. Defaulted to the same value the `.vscode` configs hardcode; an
/// existing value in the environment always wins. Only the Rust side needs it,
/// and `tauri build` passes it down to the prebuild scripts and cargo.
const BUCKET_URL_KEY: &str = "VITE_AWS_S3_BUCKET_PUBLIC_URL";
const BUCKET_URL_DEFAULT: &str = "https://explorer-artifacts.decentraland.org";

/// Tauri's CLI is a native binary behind this JS entry point. It is spawned
/// through `node` rather than `npx`/`npm run` because the `--config` argument
/// below is inline JSON: `Command` passes argv verbatim, whereas going through
/// npm re-enters `cmd.exe` on Windows and mangles the quoting.
const TAURI_CLI: &str = "node_modules/@tauri-apps/cli/tauri.js";

const PRIVATE_KEY_ENV: &str = "TAURI_SIGNING_PRIVATE_KEY";
const PRIVATE_KEY_PASSWORD_ENV: &str = "TAURI_SIGNING_PRIVATE_KEY_PASSWORD";

/// Kept out of the repo entirely, so no `.gitignore` entry can be forgotten.
const KEY_DIR_NAME: &str = "dcl-launcher-local-signing";
const PRIVATE_KEY_NAME: &str = "local.key";

fn main() {
    let root = repo_root();
    let cli = root.join(TAURI_CLI);
    if !cli.is_file() {
        fail(&format!("{} not found — run `npm install` first", cli.display()));
    }

    // On Windows `tauri.windows.conf.json` replaces `beforeBuildCommand` with
    // the sidecar prebuild, so the UI has to be built separately — the same
    // split `npm run build` already makes.
    println!("Building the UI...");
    run(&root, npm(), &["run", "build-ui"]);

    // Minted after the UI build so a failure there cannot leave a key behind.
    let key_dir = env::temp_dir().join(KEY_DIR_NAME);
    let private_key = key_dir.join(PRIVATE_KEY_NAME);
    let public_key = generate_throwaway_key(&cli, &key_dir, &private_key);

    println!("Building the bundle...");
    let status = tauri_build(&root, &cli, &private_key, &public_key);
    remove_key_dir(&key_dir);

    if !status.success() {
        exit(status.code().unwrap_or(1));
    }

    let bundle = root.join("src-tauri/target/release/bundle");
    println!("\nLocal build ready: {}", bundle.display());
    println!("Signed with a throwaway key — these artifacts are not publishable as an update.");
}

/// Mints a fresh keypair and returns the public key in the exact shape
/// `plugins.updater.pubkey` expects: the CLI already writes the `.pub` file
/// base64-encoded, so it is passed through verbatim.
fn generate_throwaway_key(cli: &Path, key_dir: &Path, private_key: &Path) -> String {
    remove_key_dir(key_dir);
    fs::create_dir_all(key_dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", key_dir.display())));

    println!("Generating a throwaway signing key...");
    // Output is captured rather than inherited: `-w` keeps the private key out
    // of the console today, and capturing keeps it that way if that ever
    // changes. `-f` overwrites, so every run really does get a new key.
    let output = Command::new("node")
        .arg(cli)
        .args(["signer", "generate", "--ci", "-p", "", "-f", "-w"])
        .arg(private_key)
        .output()
        .unwrap_or_else(|e| fail(&format!("cannot run node: {e}")));
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        fail("`tauri signer generate` failed");
    }

    let public_key = PathBuf::from(format!("{}.pub", private_key.display()));
    fs::read_to_string(&public_key)
        .unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", public_key.display())))
        .trim()
        .to_string()
}

/// Runs from the repo root, like `npm run build-core` does — Tauri resolves the
/// config's relative paths against the config file, not the working directory.
fn tauri_build(root: &Path, cli: &Path, private_key: &Path, public_key: &str) -> ExitStatus {
    // `--config` is deep-merged after `tauri.<platform>.conf.json`, so the
    // platform prebuild hooks still fire. The public key is base64, so it has
    // no characters that would need JSON escaping.
    let config = format!(r#"{{"plugins":{{"updater":{{"pubkey":"{public_key}"}}}}}}"#);

    Command::new("node")
        .arg(cli)
        .args(["build", "--config"])
        .arg(config)
        .current_dir(root)
        .env(PRIVATE_KEY_ENV, private_key)
        .env(PRIVATE_KEY_PASSWORD_ENV, "")
        .env(BUCKET_URL_KEY, bucket_url())
        .status()
        .unwrap_or_else(|e| fail(&format!("cannot run node: {e}")))
}

fn bucket_url() -> String {
    match env::var(BUCKET_URL_KEY) {
        Ok(url) if !url.is_empty() => url,
        _ => {
            println!("{BUCKET_URL_KEY} is not set, defaulting to {BUCKET_URL_DEFAULT}");
            BUCKET_URL_DEFAULT.to_string()
        }
    }
}

fn remove_key_dir(key_dir: &Path) {
    if let Err(e) = fs::remove_dir_all(key_dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            fail(&format!("cannot remove {}: {e}", key_dir.display()));
        }
    }
}

fn npm() -> &'static str {
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}

include!("shared.rs");
