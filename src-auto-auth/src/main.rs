// Avoid popup terminal window
#![windows_subsystem = "windows"]

use std::path::Path;

use dcl_launcher_core::{
    anyhow::{Context, Result, anyhow},
    auto_auth::anon_user_id::AnonUserId,
    auto_auth::auth_token_storage::AuthTokenStorage,
    auto_auth::campaign_anon_user_id_storage::CampaignAnonUserIdStorage,
    log, logs,
};

/// Filename prefix the download gateway uses when serving the anonymous EXE.
/// The full filename is `<INSTALLER_FILENAME_PREFIX><UUID>.exe` (with an
/// optional ` (n)` suffix added by the browser when deduplicating downloads
/// in the user's Downloads folder).
const INSTALLER_FILENAME_PREFIX: &str = "Decentraland-Installer-";

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

    let args: Vec<String> = std::env::args().collect();
    log::info!("Args: {args:?}");

    let installer_path = args
        .last()
        .ok_or_else(|| anyhow!("Installer path is not provided"))?;
    log::info!("Installer path: {installer_path}");

    if CampaignAnonUserIdStorage::has() {
        log::info!("Campaign anon_user_id already present in storage");
    } else if let Some(anon_id) = extract_anon_user_id_from_zone(installer_path) {
        log::info!("Campaign anon_user_id extracted from Zone.Identifier");
        if let Err(e) = CampaignAnonUserIdStorage::write(&anon_id) {
            log::error!("Cannot write campaign anon user id: {e}");
        }
    } else if let Some(anon_id) = extract_anon_user_id_from_filename(installer_path) {
        // Fallback for the anonymous Download First flow on Windows: the
        // gateway encodes the UUID in the Content-Disposition filename so
        // attribution survives Windows' silent-unblock-on-launch handling
        // (which strips the Zone.Identifier ADS for trusted signed binaries
        // before this script runs).
        log::info!("Campaign anon_user_id extracted from filename");
        if let Err(e) = CampaignAnonUserIdStorage::write(&anon_id) {
            log::error!("Cannot write campaign anon user id: {e}");
        }
    } else {
        log::info!("No campaign anon_user_id found in Zone.Identifier URLs or installer filename");
    }

    if AuthTokenStorage::has_token() {
        log::info!("Token already installed");
        return Ok(());
    }

    let token = token_from_file_by_zone_attr(installer_path)?;
    AuthTokenStorage::write_token(token.as_str())?;
    log::info!("Token write complete");
    Ok(())
}

/// Try to extract `anon_user_id` from Zone.Identifier URLs.
fn extract_anon_user_id_from_zone(installer_path: &str) -> Option<AnonUserId> {
    let content = match zone_identifier_content(installer_path).or_else(|e| {
        log::error!("ADS read for anon_user_id failed via CAPI, fallback to PowerShell: {e:?}");
        zone_identifier_content_powershell(installer_path)
    }) {
        Ok(c) => c,
        Err(e) => {
            log::error!("Cannot read Zone.Identifier for anon_user_id: {e:?}");
            return None;
        }
    };

    let zone_info = parsed_zone_identifier(&content);

    [zone_info.host_url.as_deref(), zone_info.referrer_url.as_deref()]
        .into_iter()
        .flatten()
        .find_map(AnonUserId::from_url)
}

/// Try to extract `anon_user_id` from the installer's filename.
///
/// The download gateway names anonymous EXE downloads
/// `Decentraland-Installer-<UUID>.exe`. When the user already has a file with
/// the same name in `Downloads`, browsers append a ` (n)` dedup suffix
/// (e.g. `Decentraland-Installer-<UUID> (3).exe`); we tolerate that.
///
/// This is the fallback path used when Zone.Identifier has been stripped by
/// Windows' silent-unblock handling for trusted signed binaries — which is
/// the steady-state for popular pre-signed installers and not an edge case.
fn extract_anon_user_id_from_filename(installer_path: &str) -> Option<AnonUserId> {
    let stem = Path::new(installer_path).file_stem()?.to_str()?;
    let after_prefix = stem.strip_prefix(INSTALLER_FILENAME_PREFIX)?;
    // Strip the browser's " (n)" dedup suffix if present.
    let cleaned = after_prefix
        .split_once(" (")
        .map_or(after_prefix, |(before, _)| before);
    AnonUserId::parse(cleaned)
}

// Zone.Identifier
fn token_from_file_by_zone_attr(path: &str) -> Result<String> {
    let content = zone_identifier_content(path)
        .or_else(|e| {
            log::error!("ADS read from direct CAPI failed, fallback to PowerShell: {e:?}");
            zone_identifier_content_powershell(path)
        })
        .with_context(|| {
            anyhow!("Reading zone content from both CAPI and PowerShell failed for file '{path}'")
        })?;
    let content = parsed_zone_identifier(&content);
    token_from_zone_info(content)
}

fn token_from_zone_info(zone_info: ZoneInfo) -> Result<String> {
    use dcl_launcher_core::auto_auth::DownloadOriginData;

    for url in [zone_info.host_url.as_deref(), zone_info.referrer_url.as_deref()]
        .into_iter()
        .flatten()
    {
        if let Ok(origin) = DownloadOriginData::from_url(url) {
            if let Some(token) = origin.auth_token {
                return Ok(token);
            }
        }
    }

    Err(anyhow!(
        "Token not found in Zone.Identifier attribute: {zone_info:?}"
    ))
}

#[allow(unsafe_code)]
#[cfg(windows)]
fn log_alternate_data_streams(path: &str) -> Result<()> {
    use std::ffi::OsStr;
    use std::ffi::c_void;
    use std::os::windows::prelude::*;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Storage::FileSystem::*;

    let w_path: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
    let mut stream_data: WIN32_FIND_STREAM_DATA = unsafe { std::mem::zeroed() };

    log::info!("Starting ADS enumeration for file: {path}");

    unsafe {
        let stream_ptr = &mut stream_data as *mut _ as *mut c_void;
        let h_find_stream = FindFirstStreamW(
            w_path.as_ptr(),
            FindStreamInfoStandard,
            stream_ptr,
            0, // dwFlags, reserved, must be 0
        );

        if h_find_stream == INVALID_HANDLE_VALUE {
            let error = std::io::Error::last_os_error();
            return Err(anyhow!("FindFirstStreamW failed: {error:?}"));
        }

        loop {
            let stream_name_wide = &stream_data.cStreamName;

            let name_len = stream_name_wide.iter().take_while(|&c| *c != 0).count();
            let name = String::from_utf16_lossy(&stream_name_wide[..name_len]);

            let size = stream_data.StreamSize;

            log::info!("Found Stream: Name='{name}', Size={size} bytes");

            // Continue to the next stream
            if FindNextStreamW(h_find_stream, stream_ptr) == 0 {
                // FindNextStreamW returns 0 (FALSE) when no more streams are found or an error occurs
                let last_error = GetLastError();
                if last_error != ERROR_NO_MORE_FILES {
                    log::warn!("FindNextStreamW encountered an unexpected error: {last_error}");
                }
                break; // Exit the loop
            }
        }

        CloseHandle(h_find_stream);
    }

    log::info!("Finished ADS enumeration for file: {path}");
    Ok(())
}

#[cfg(windows)]
fn ads_content(path: &str) -> Result<Vec<u8>> {
    use std::ffi::OsStr;
    use std::os::windows::prelude::*;
    use std::ptr;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Storage::FileSystem::*;

    let original_files_exists = std::fs::exists(path).context("Error checking original file")?;

    if !original_files_exists {
        return Err(anyhow!("Original file does not exist: {path}"));
    }

    let ads_path = format!("{path}:Zone.Identifier");
    log::info!("Opening ads info of: {ads_path}");
    let w: Vec<u16> = OsStr::new(&ads_path).encode_wide().chain(Some(0)).collect();

    if let Err(e) = log_alternate_data_streams(path) {
        log::error!("Cannot log ads list: {e:?}");
    }

    #[allow(unsafe_code)]
    unsafe {
        let handle = CreateFileW(
            w.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut::<std::ffi::c_void>(),
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

fn zone_identifier_content_powershell(path: &str) -> Result<String> {
    use std::process::{Command, Stdio};

    let output = Command::new("powershell.exe")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(format!(
            "Get-Content -Path '{}' -Stream Zone.Identifier",
            path.replace("'", "''") // escape single quotes
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        return Err(anyhow!(
            "PowerShell failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

    // Tests use forward-slash paths so they run on both Windows and Unix CI
    // hosts. `Path::file_stem` uses the host OS's path semantics, but the
    // production code targets Windows where `\` and `/` both work as
    // separators — and we only need to exercise the parsing logic here, not
    // the OS path resolution.
    #[rstest]
    #[case(
        "Downloads/Decentraland-Installer-391a85da-a3bb-49e2-a45e-96c740c38424.exe",
        Some("391a85da-a3bb-49e2-a45e-96c740c38424")
    )]
    #[case(
        // Bare filename, no parent directory.
        "Decentraland-Installer-391a85da-a3bb-49e2-a45e-96c740c38424.exe",
        Some("391a85da-a3bb-49e2-a45e-96c740c38424")
    )]
    #[case(
        // Browser dedup suffix when the file already exists in Downloads.
        "Decentraland-Installer-391a85da-a3bb-49e2-a45e-96c740c38424 (3).exe",
        Some("391a85da-a3bb-49e2-a45e-96c740c38424")
    )]
    #[case(
        // Different valid UUID, slash-prefixed absolute path.
        "/tmp/Decentraland-Installer-62792c33-59e3-4e7f-be42-289c053ecb37.exe",
        Some("62792c33-59e3-4e7f-be42-289c053ecb37")
    )]
    #[case(
        // Old-style filename (no UUID) → no fallback match, the caller must
        // treat this as "no anon_user_id available".
        "Decentraland-Installer.exe",
        None
    )]
    #[case(
        // Wrong prefix (different launcher build) → no match.
        "some-other-installer-391a85da-a3bb-49e2-a45e-96c740c38424.exe",
        None
    )]
    #[case(
        // Prefix matches but the UUID part contains a character AnonUserId
        // rejects (raw space). Defends against malformed filenames written by
        // an attacker who controls the download URL.
        "Decentraland-Installer-not a uuid.exe",
        None
    )]
    #[case(
        // Empty stem (impossible in practice, but we should not panic).
        "",
        None
    )]
    fn test_extract_anon_user_id_from_filename(#[case] path: &str, #[case] expected: Option<&str>) {
        let actual = extract_anon_user_id_from_filename(path);
        assert_eq!(expected, actual.as_ref().map(|id| id.as_str()));
    }
}
