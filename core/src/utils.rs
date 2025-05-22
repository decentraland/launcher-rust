use regex::Regex;
use std::collections::HashMap;
use std::env;

pub fn get_os_name() -> &'static str {
    let os = std::env::consts::OS;
    match os {
        "macos" => os,
        "linux" => os,
        "windows" => "windows64",
        _ => "unsupported",
    }
}

pub fn parsed_argv() -> HashMap<String, String> {
    let mut parsed_argv = HashMap::new();
    let args: Vec<String> = env::args().collect();

    for arg in &args {
        if let Some((key, value)) = arg.strip_prefix("--").and_then(|s| s.split_once('=')) {
            match key {
                "version" | "prerelease" | "dev" | "downloadedfilepath" => {
                    parsed_argv.insert(key.to_string(), value.to_string());
                }
                _ => {}
            }
        } else if let Some(key) = arg.strip_prefix("--") {
            match key {
                "version" | "prerelease" | "dev" | "downloadedfilepath" => {
                    parsed_argv.insert(key.to_string(), "true".to_string());
                }
                _ => {}
            }
        }
    }

    parsed_argv
}

fn is_valid_version(version: &str) -> bool {
    let version_regex = Regex::new(r"^(v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?)|dev$")
        .ok();

    match version_regex {
        Some(regex) => regex.is_match(version),
        None => false, // If regex fails to compile, return false as a fallback
    }
}

/**
 * Retrieves the version from the parsed command-line arguments.
 * @returns The version string if available, otherwise undefined.
 */
pub fn get_version(args: &HashMap<String, String>) -> Option<&String> {
    if let Some(version) = args.get("version") {
        if is_valid_version(version.as_str()) {
            return Some(version);
        }
    }

    return None;
}

pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/**
 * Determines if should run the dev version of the Explorer when passing the arguments --dev or --version=dev.
 * @returns A boolean value indicating if the Explorer should run the explorer from the dev path.
 */
pub fn should_run_dev_version(args: &HashMap<String, String>) -> bool {
    if let Some(value) = args.get("dev") {
        return value == "true";
    }
    if let Some(value) = args.get("version") {
        return value == "dev";
    }
    if let Some(_) = args.get("downloadedfilepath") {
        return true;
    }
    return false;
}

/*
 * Determines if should download a prerelease version of the Explorer.
 * @returns A boolean value indicating if the Explorer should be downloaded as a prerelease version.
 */
pub fn is_prerelease(args: &HashMap<String, String>) -> bool {
    if let Some(value) = args.get("prerelease") {
        return value == "true";
    }

    return false;
}

/*
 * Retrieves the downloaded file path from the parsed command-line arguments.
 * @returns The downloaded file path string if available, otherwise undefined.
 */
pub fn downloaded_file_path(args: &HashMap<String, String>) -> Option<&String> {
    args.get("downloadedfilepath")
}
