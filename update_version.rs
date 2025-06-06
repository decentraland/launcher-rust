#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! serde = { version = "1.0", features = ["derive"] }
//! serde_json = { version = "1.0", features = ["preserve_order"] }
//! toml_edit = "0.22"
//! semver = "1.0"
//! anyhow = "1.0"
//! ```

use anyhow::{Context, Result, anyhow};
use semver::{Version};
use std::{env, fs};
use toml_edit::{DocumentMut, value};

const PACKAGE_JSON: &str = "package.json";
const PACKAGE_JSON_LOCK: &str = "package-lock.json";
const APP_CONFIG_LOCK: &str = "src-tauri/tauri.conf.json";
const APP_RS_TOML: &str = "src-tauri/Cargo.toml";
const CORE_RS_TOML: &str = "core/Cargo.toml";

#[derive(Debug)]
enum BumpType {
    Major,
    Minor,
    Patch,
}

impl BumpType {
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "major" => Ok(BumpType::Major),
            "minor" => Ok(BumpType::Minor),
            "patch" => Ok(BumpType::Patch),
            _ => Err(anyhow!("Bump type '{}' is not recognized, use [patch|minor|major]", s))
        }
    }

    fn bump(&self, mut version: Version) -> Version {
        match self {
            BumpType::Major => {
                version.major += 1;
                version.minor = 0;
                version.patch = 0;
            }
            BumpType::Minor => {
                version.minor += 1;
                version.patch = 0;
            }
            BumpType::Patch => {
                version.patch += 1;
            }
        }
        version
    }
}

fn update_json_version(path: &str, bump: &BumpType) -> Result<()> {
    let raw = fs::read_to_string(path).context("Failed to read JSON")?;
    let mut json: serde_json::Value = serde_json::from_str(&raw)?;

    let old_version = json["version"].as_str().context("Cannot get version field")?.to_owned();
    let parsed = Version::parse(&old_version)?;
    let new_version = bump.bump(parsed);

    json["version"] = serde_json::Value::String(new_version.to_string());

    fs::write(path, serde_json::to_string_pretty(&json)?)?;
    println!("Updated version {path}: {old_version} → {new_version}");
    Ok(())
}

fn update_toml_version(path: &str, bump: &BumpType) -> Result<()> {
    let raw = fs::read_to_string(path).context("Failed to read TOML")?;
    let mut doc: DocumentMut = raw.parse().context("Failed to parse TOML")?;

    let old_version = doc["package"]["version"].as_str().context("Cannot get version field")?.to_owned();
    let parsed = Version::parse(&old_version)?;
    let new_version = bump.bump(parsed);

    doc["package"]["version"] = value(new_version.to_string());
    fs::write(path, doc.to_string())?;
    println!("Updated version {path}: {old_version} → {new_version}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run -- [patch|minor|major] or use directly ./update_version.rs [patch|minor|major]");
        return Ok(());
    }

    let bump = BumpType::from_str(&args[1])?;

    update_json_version(PACKAGE_JSON, &bump).context("Cannot bump PACKAGE_JSON")?;
    update_json_version(PACKAGE_JSON_LOCK, &bump).context("Cannot bump PACKAGE_JSON_LOCK")?;
    update_json_version(APP_CONFIG_LOCK, &bump).context("Cannot bump APP_CONFIG_LOCK")?;
    update_toml_version(APP_RS_TOML, &bump).context("Cannot bump APP_RS_TOML")?;
    update_toml_version(CORE_RS_TOML, &bump).context("Cannot bump CORE_RS_TOML")?;

    Ok(())
}

