#!/usr/bin/env rust-script
//! Builds every sidecar/resource binary the Windows bundle needs, in order.
//!
//! Usage: `rust-script scripts/pre-build-sidecars.rs`
//!
//! Each step is delegated to its own script so the individual steps stay
//! usable on their own (CI runs them separately).

const SCRIPT_NAME: &str = "pre-build-sidecars";
const STEPS: [&str; 2] = ["scripts/pre-build-installer-hooks.rs", "scripts/pre-build-service.rs"];

fn main() {
    let root = repo_root();
    for step in STEPS {
        rust_script(&root, step);
    }
}

include!("shared.rs");
