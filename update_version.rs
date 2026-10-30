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

// Single source of truth for versioned manifests, shared with .github/workflows/bump-version.yml
const VERSION_FILES_JSON: &str = "version-files.json";

#[derive(Debug)]
struct VersionPair {
    #[allow(dead_code)]
    path: String,
    version: Version,
}

fn version_files() -> Result<Vec<String>> {
    let raw = fs::read_to_string(VERSION_FILES_JSON).context("Failed to read version files list")?;
    let files: Vec<String> = serde_json::from_str(&raw).context("Failed to parse version files list")?;
    anyhow::ensure!(!files.is_empty(), "{} lists no files", VERSION_FILES_JSON);
    Ok(files)
}

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

fn update_version(path: &str, bump: &BumpType) -> Result<()> {
    if path.ends_with(".json") {
        update_json_version(path, bump)
    }
    else if path.ends_with(".toml") {
        update_toml_version(path, bump)
    }
    else {
        Err(anyhow!("File type of {} is unsupported", path))
    }
}

fn version_from_toml(path: &str) -> Result<Version> {
    let raw = fs::read_to_string(path).context("Failed to read TOML")?;
    let doc: DocumentMut = raw.parse().context("Failed to parse TOML")?;
    let version = doc["package"]["version"].as_str().context("Cannot get version field")?.to_owned();
    let parsed = Version::parse(&version)?;
    Ok(parsed)
}

fn version_from_json(path: &str) -> Result<Version> {
    let raw = fs::read_to_string(path).context("Failed to read JSON")?;
    let json: serde_json::Value = serde_json::from_str(&raw)?;
    let old_version = json["version"].as_str().context("Cannot get version field")?.to_owned();
    let parsed = Version::parse(&old_version)?;
    Ok(parsed)
}

fn version_from(path: &str) -> Result<Version> {
    if path.ends_with(".json") {
        version_from_json(path)
    }
    else if path.ends_with(".toml") {
        version_from_toml(path)
    }
    else {
        Err(anyhow!("File type of {} is unsupported", path))
    }
}

fn ensure_versions_are_equal(versions: Vec<VersionPair>) -> Result<()> {
    match versions.first() {
        Some(first) => {
            if versions.iter().any(|v| v.version != first.version) {
                Err(anyhow!("Not all versions are equal: {:#?}", versions))
            }
            else {
                Ok(())
            }
        },
        None => {
            Ok(())
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run -- [patch|minor|major|check] or use directly ./update_version.rs [patch|minor|major|check]");
        return Ok(());
    }

    // `check` only verifies that all manifests share one version, without bumping
    let bump = match args[1].as_str() {
        "check" => None,
        other => Some(BumpType::from_str(other)?),
    };

    let files = version_files()?;

    let versions = files
        .iter()
        .map(|path| {
            let version = version_from(path)?;
            Ok(VersionPair { path: path.clone(), version })
        })
        .collect::<Result<_>>()?;


    ensure_versions_are_equal(versions)?;

    let Some(bump) = bump else {
        println!("All versions are equal");
        return Ok(());
    };

    for path in &files {
        update_version(path, &bump)
            .with_context(|| format!("Cannot bump version in {}", path))?;
    }

    Ok(())
}

