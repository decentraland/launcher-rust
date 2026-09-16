#!/usr/bin/env rust-script
//! Builds every sidecar binary the bundle needs, in order.
//!
//! Usage: `rust-script scripts/pre-build-sidecars.rs`
//!
//! Each step is delegated to its own script so the individual steps stay
//! usable on their own (CI runs them separately).

const SCRIPT_NAME: &str = "pre-build-sidecars";
const STEPS: [&str; 1] = ["scripts/pre-build-watchdog.rs"];

fn main() {
    let root = repo_root();
    for step in STEPS {
        rust_script(&root, step);
    }
}

include!("shared.rs");
