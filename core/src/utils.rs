use regex::Regex;
use std::collections::HashMap;
use std::env;

#[must_use]
pub fn get_os_name() -> &'static str {
    let os = std::env::consts::OS;
    match os {
        "macos" | "linux" => os,
        "windows" => "windows64",
        _ => "unsupported",
    }
}

fn is_valid_version(version: &str) -> bool {
    let version_regex = Regex::new(r"^(v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?)|dev$")
        .ok();

    // If regex fails to compile, return false as a fallback
    version_regex.is_some_and(|regex| regex.is_match(version))
}

/**
 * Retrieves the version from the parsed command-line arguments.
 * @returns The version string if available, otherwise undefined.
 */
#[allow(clippy::implicit_hasher)]
#[must_use]
pub fn get_version(args: &HashMap<String, String>) -> Option<&String> {
    if let Some(version) = args.get("version") {
        if is_valid_version(version.as_str()) {
            return Some(version);
        }
    }

    None
}

#[must_use]
pub const fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/**
 * Determines if should run the dev version of the Explorer when passing the arguments --dev or --version=dev.
 * @returns A boolean value indicating if the Explorer should run the explorer from the dev path.
 */
#[allow(clippy::implicit_hasher)]
#[must_use]
pub fn should_run_dev_version(args: &HashMap<String, String>) -> bool {
    if let Some(value) = args.get("dev") {
        return value == "true";
    }
    if let Some(value) = args.get("version") {
        return value == "dev";
    }
    if args.get("downloadedfilepath").is_some() {
        return true;
    }

    false
}

/*
 * Determines if should download a prerelease version of the Explorer.
 * @returns A boolean value indicating if the Explorer should be downloaded as a prerelease version.
 */
#[allow(clippy::implicit_hasher)]
#[must_use]
pub fn is_prerelease(args: &HashMap<String, String>) -> bool {
    if let Some(value) = args.get("prerelease") {
        return value == "true";
    }

    false
}

/*
 * Retrieves the downloaded file path from the parsed command-line arguments.
 * @returns The downloaded file path string if available, otherwise undefined.
 */
#[allow(clippy::implicit_hasher)]
#[must_use]
pub fn downloaded_file_path(args: &HashMap<String, String>) -> Option<&String> {
    args.get("downloadedfilepath")
}
