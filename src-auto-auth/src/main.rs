// Avoid popup terminal window
#![windows_subsystem = "windows"]

use std::{fs::File, io::Read, process::exit};

use dcl_launcher_core::{
    anyhow::{Context, Result, anyhow},
    auto_auth::auth_token_storage::AuthTokenStorage,
    log, logs,
};

#[derive(Debug, Default)]
pub struct ZoneInfo {
    pub zone_id: Option<u32>,
    pub host_url: Option<String>,
    pub referrer_url: Option<String>,
}

fn main() {
    if let Err(e) = logs::dispath_logs() {
        eprintln!("Cannot initialize logs: {e}");
        exit(1);
    }
    if let Err(e) = main_internal() {
        log::error!("Error occurred running auto auth script: {e:?}");
    }
}

fn main_internal() -> Result<()> {
    log::info!("Start auto auth script v{}", std::env!("CARGO_PKG_VERSION"));
    if AuthTokenStorage::has_token() {
        log::info!("Token already installed");
        return Ok(());
    }

    let args: Vec<String> = std::env::args().collect();
    log::info!("Args: {args:?}");

    let installer_path = args
        .last()
        .ok_or_else(|| anyhow!("Installer path is not provided"))?;
    log::info!("Installer path: {installer_path}");

    let token = token_from_file_by_zone_attr(installer_path)?;
    AuthTokenStorage::write_token(token.as_str())?;
    log::info!("Token write complete");
    Ok(())
}

// Zone.Identifier
fn token_from_file_by_zone_attr(path: &str) -> Result<String> {
    let content = zone_identifier_content(path)?;
    let content = parsed_zone_identifier(&content);

    if let Some(url) = &content.host_url {
        let token = dcl_launcher_core::auto_auth::token_from_url(url)?;
        if let Some(token) = token {
            return Ok(token);
        }
    }

    if let Some(url) = &content.referrer_url {
        let token = dcl_launcher_core::auto_auth::token_from_url(url)?;
        if let Some(token) = token {
            return Ok(token);
        }
    }

    Err(anyhow!("Token not found in Zone.Identifier: {content:?}"))
}

fn to_verbatim(p: &str) -> String {
    if p.starts_with(r"\\?\") {
        p.to_string()
    } else {
        format!(r"\\?\{p}")
    }
}

fn zone_identifier_content(path: &str) -> Result<String> {
    let original_files_exists = std::fs::exists(path).context("Error checking original file")?;

    if !original_files_exists {
        return Err(anyhow!("Original file does not exist: {path}"));
    }

    let ads_path = format!("{path}:Zone.Identifier");
    let ads_path = to_verbatim(&ads_path);

    // Try to open ADS
    log::info!("Opening ads info of: {ads_path}");
    let mut file =
        File::open(&ads_path).context("File doesn't have ADS to read the Zone.Identifier from")?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    if buf.is_empty() {
        return Err(anyhow!("ADS is empty"));
    }

    // CASE 1: UTF-16 LE with BOM FFFE
    if buf.starts_with(&[0xFF, 0xFE]) {
        use std::char::decode_utf16;

        // strip BOM and decode
        let words = buf[2..]
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));

        let decoded: String = decode_utf16(words)
            .map(|r| r.unwrap_or('\u{FFFD}'))
            .collect();

        return Ok(decoded);
    }

    // CASE 2: UTF-16 LE but WITHOUT BOM
    // Most Windows components write UTF-16 LE by default.
    if buf.len() % 2 == 0 {
        let mut looks_utf16 = true;
        for chunk in buf.chunks(2) {
            if chunk.len() != 2 {
                looks_utf16 = false;
                break;
            }
        }

        if looks_utf16 {
            use std::char::decode_utf16;
            let words = buf
                .chunks(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));

            let decoded: String = decode_utf16(words)
                .map(|r| r.unwrap_or('\u{FFFD}'))
                .collect();

            // Heuristic: INI file must contain ASCII printable characters
            if decoded.contains("ZoneTransfer") || decoded.contains("ZoneId") {
                return Ok(decoded);
            }
        }
    }

    // CASE 3: Assume UTF-8 / ANSI
    let text = String::from_utf8_lossy(&buf).to_string();
    Ok(text)
}

fn parsed_zone_identifier(contents: &str) -> ZoneInfo {
    let mut info = ZoneInfo::default();

    for line in contents.lines() {
        let line = line.trim();

        // Skip section header
        if line.starts_with('[') && line.ends_with(']') {
            continue;
        }

        // Split on first '='
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_string();

        match key.as_str() {
            "zoneid" => {
                if let Ok(id) = value.parse::<u32>() {
                    info.zone_id = Some(id);
                }
            }
            "hosturl" => {
                info.host_url = Some(value);
            }
            "referrerurl" => {
                info.referrer_url = Some(value);
            }
            _ => {}
        }
    }

    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcl_launcher_core::anyhow::Result;

    #[test]
    fn test_integration_token_from_file() -> Result<()> {
        let file_path = option_env!("EXE_WITH_TOKEN");
        let Some(path) = file_path else {
            println!("no env var provided EXE_WITH_TOKEN");
            return Ok(());
        };

        let token = token_from_file_by_zone_attr(path)?;
        println!("{token}");
        Ok(())
    }

    #[test]
    fn test_integration_read_ads() -> Result<()> {
        let file_path = option_env!("EXE_WITH_TOKEN");
        let Some(path) = file_path else {
            println!("no env var provided EXE_WITH_TOKEN");
            return Ok(());
        };

        let content = zone_identifier_content(path)?;
        println!("{content}");
        Ok(())
    }
}
