// Avoid popup terminal window
#![windows_subsystem = "windows"]

#[cfg(unix)]
use dcl_launcher_core::{
    anyhow::{Result, anyhow},
    auto_auth::auth_token_storage::AuthTokenStorage,
    log, logs,
};

#[cfg(windows)]
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
        std::process::exit(1);
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
    token_from_zone_info(content)
}

fn token_from_zone_info(zone_info: ZoneInfo) -> Result<String> {
    if let Some(url) = &zone_info.host_url {
        let token = dcl_launcher_core::auto_auth::token_from_url(url)?;
        if let Some(token) = token {
            return Ok(token);
        }
    }

    if let Some(url) = &zone_info.referrer_url {
        let token = dcl_launcher_core::auto_auth::token_from_url(url)?;
        if let Some(token) = token {
            return Ok(token);
        }
    }

    Err(anyhow!(
        "Token not found in Zone.Identifier attribute: {zone_info:?}"
    ))
}

#[cfg(windows)]
fn ads_content(path: &str) -> Result<Vec<u8>> {
    use std::ffi::OsStr;
    use std::os::windows::prelude::*;
    use std::ptr;
    use std::{fs::File, io::Read};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Storage::FileSystem::*;

    let original_files_exists = std::fs::exists(path).context("Error checking original file")?;

    if !original_files_exists {
        return Err(anyhow!("Original file does not exist: {path}"));
    }

    let ads_path = format!("{path}:Zone.Identifier");
    log::info!("Opening ads info of: {ads_path}");
    let w: Vec<u16> = OsStr::new(&ads_path).encode_wide().chain(Some(0)).collect();

    #[allow(unsafe_code)]
    unsafe {
        let handle = CreateFileW(
            w.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0,
        );

        if handle == INVALID_HANDLE_VALUE {
            let error = std::io::Error::last_os_error();
            return Err(anyhow!("Open file failed CreateFileW: {error:?}"));
        }

        let mut buf = vec![0u8; 16384];
        let mut bytes_read = 0u32;

        let success = ReadFile(
            handle,
            buf.as_mut_ptr() as *mut _,
            buf.len() as u32,
            &mut bytes_read,
            ptr::null_mut(),
        );

        CloseHandle(handle);

        if success == 0 {
            let error = std::io::Error::last_os_error();
            return Err(anyhow!("Read failed ReadFile: {error:?}"));
        }

        buf.truncate(bytes_read as usize);
        Ok(buf)
    }
}

#[cfg(unix)]
fn ads_content(_path: &str) -> Result<Vec<u8>> {
    Err(anyhow!("ADS is not supported on macOS"))
}

fn zone_identifier_content(path: &str) -> Result<String> {
    let buf = ads_content(path)?;

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
    use rstest::rstest;

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

    #[rstest]
    #[case(
        "https://example.com/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg",
        "391a85da-a3bb-49e2-a45e-96c740c38424"
    )]
    #[case(
        "https://example.com/subpath/run-855-19672401394/Decentraland_installer.exe?token=b5876cf1-9b6b-451e-b467-9700f754a8f7",
        "b5876cf1-9b6b-451e-b467-9700f754a8f7"
    )]
    fn test_token_from_url(
        #[case] zone_info_url: &str,
        #[case] expected_token: &str,
    ) -> Result<()> {
        let zone = ZoneInfo {
            host_url: Some(zone_info_url.to_owned()),
            ..Default::default()
        };

        let token = token_from_zone_info(zone)?;
        assert_eq!(expected_token, token.as_str());
        Ok(())
    }
}
