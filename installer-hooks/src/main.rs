// Avoid popup terminal window
#![windows_subsystem = "windows"]

use std::path::Path;
use std::time::Duration;

use dcl_launcher_core::{
    analytics::{Analytics, event::Event},
    anyhow::{Context, Result, anyhow},
    download_origin_metadata::DownloadOriginData,
    download_origin_metadata::anon_user_id::AnonUserId,
    download_origin_metadata::auth_token_storage::AuthTokenStorage,
    download_origin_metadata::campaign_anon_user_id_storage::CampaignAnonUserIdStorage,
    download_origin_metadata::dcl_env_storage::DclEnvStorage,
    download_origin_metadata::referrer_storage::ReferrerStorage,
    download_origin_metadata::startup_location_storage::StartupDeeplinkStorage,
    log, logs,
};

const EVENT_SEND_TIMEOUT: Duration = Duration::from_secs(5);

const INSTALLER_EVENT_COMMAND: &str = "installer-event";

#[derive(Debug, Default)]
pub struct ZoneInfo {
    pub zone_id: Option<u32>,
    pub host_url: Option<String>,
    pub referrer_url: Option<String>,
}

enum Command {
    AuthToken {
        installer_path: String,
    },
    InstallerEvent {
        phase: InstallerPhase,
        installer_path: String,
    },
}

enum InstallerPhase {
    Start,
    Finish,
}

impl Command {
    fn parse(args: &[String]) -> Result<Self> {
        match args.get(1).map(String::as_str) {
            Some(INSTALLER_EVENT_COMMAND) => {
                let phase = args
                    .get(2)
                    .ok_or_else(|| anyhow!("Installer event phase is not provided"))?;
                let installer_path = args
                    .get(3)
                    .ok_or_else(|| anyhow!("Installer path is not provided"))?;
                Ok(Self::InstallerEvent {
                    phase: InstallerPhase::parse(phase)?,
                    installer_path: installer_path.clone(),
                })
            }
            Some(installer_path) => Ok(Self::AuthToken {
                installer_path: installer_path.to_owned(),
            }),
            None => Err(anyhow!("Installer path is not provided")),
        }
    }
}

impl InstallerPhase {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "start" => Ok(Self::Start),
            "finish" => Ok(Self::Finish),
            other => Err(anyhow!("Unknown installer event phase '{other}'")),
        }
    }

    fn into_event(self, installer_file_name: String) -> Event {
        match self {
            Self::Start => Event::LAUNCHER_INSTALLER_START {
                installer_file_name,
            },
            Self::Finish => Event::LAUNCHER_INSTALLER_FINISH {
                installer_file_name,
            },
        }
    }
}

fn main() {
    if let Err(e) = logs::dispath_logs() {
        eprintln!("Cannot initialize logs: {e}");
        std::process::exit(1);
    }
    if let Err(e) = main_internal() {
        log::error!("Error occurred running installer hooks: {e:?}");
    }
}

fn main_internal() -> Result<()> {
    log::info!("Start installer hooks v{}", std::env!("CARGO_PKG_VERSION"));

    let args: Vec<String> = std::env::args().collect();
    log::info!("Args: {args:?}");

    match Command::parse(&args)? {
        Command::AuthToken { installer_path } => run_auth_token(&installer_path),
        Command::InstallerEvent {
            phase,
            installer_path,
        } => run_installer_event(phase, &installer_path),
    }
}

fn run_installer_event(phase: InstallerPhase, installer_path: &str) -> Result<()> {
    log::info!("Installer path: {installer_path}");

    let installer_file_name = Path::new(installer_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();

    let campaign_anon_user_id = campaign_anon_user_id_for_event(installer_path);

    match &campaign_anon_user_id {
        Some(id) => log::info!("Campaign anon_user_id for installer event: {id}"),
        None => log::info!("No campaign anon_user_id available for installer event"),
    }

    let event = phase.into_event(installer_file_name);
    log::info!("Tracking installer event: {event}");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Cannot build tokio runtime for installer event")?;

    runtime.block_on(async move {
        let mut analytics = Analytics::new_from_env();
        if let Some(id) = &campaign_anon_user_id {
            analytics = analytics.with_campaign_anon_user_id(id.as_str());
        }
        analytics.track_and_flush_silent(event).await;
        analytics.cleanup_within(EVENT_SEND_TIMEOUT).await;
    });

    log::info!("Installer event complete");
    Ok(())
}

fn campaign_anon_user_id_for_event(installer_path: &str) -> Option<AnonUserId> {
    let from_zone = read_zone_origin(installer_path)
        .inspect_err(|e| {
            log::error!("Cannot read Zone.Identifier download-origin metadata: {e:?}");
        })
        .ok()
        .and_then(|origin| origin.campaign_anon_user_id);

    from_zone
        .or_else(|| AnonUserId::from_installer_filename(installer_path))
        .or_else(CampaignAnonUserIdStorage::read)
}

fn run_auth_token(installer_path: &str) -> Result<()> {
    log::info!("Installer path: {installer_path}");

    let origin = read_zone_origin(installer_path);
    if let Err(e) = &origin {
        log::error!("Cannot read Zone.Identifier download-origin metadata: {e:?}");
    }
    let origin = origin.ok();

    if CampaignAnonUserIdStorage::has() {
        log::info!("Campaign anon_user_id already present in storage");
    } else if let Some(anon_id) = origin
        .as_ref()
        .and_then(|o| o.campaign_anon_user_id.as_ref())
    {
        log::info!("Campaign anon_user_id extracted from Zone.Identifier");
        if let Err(e) = CampaignAnonUserIdStorage::write(anon_id) {
            log::error!("Cannot write campaign anon user id: {e}");
        }
    } else if let Some(anon_id) = AnonUserId::from_installer_filename(installer_path) {
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

    // Referrer extraction must happen before the auth-token early return below:
    // reinstalls on a machine that already has a token would otherwise skip it.
    if ReferrerStorage::has() {
        log::info!("Referrer already present in storage");
    } else if let Some(referrer) = origin.as_ref().and_then(|o| o.referrer.as_ref()) {
        log::info!("Referrer extracted from Zone.Identifier");
        if let Err(e) = ReferrerStorage::write(referrer) {
            log::error!("Cannot write referrer: {e}");
        }
    } else {
        log::info!("No referrer found in Zone.Identifier URLs");
    }

    if let Some(dcl_env) = origin.as_ref().and_then(|o| o.dcl_env) {
        log::info!("Environment extracted from Zone.Identifier: {dcl_env}");
        if let Err(e) = DclEnvStorage::write(dcl_env) {
            log::error!("Cannot write dcl environment: {e}");
        }
    } else {
        log::info!("No environment found in Zone.Identifier URLs");
    }

    if !StartupDeeplinkStorage::has() {
        if let Some(deeplink) = origin.as_ref().and_then(|o| o.to_startup_deeplink()) {
            log::info!(
                "Persisting startup location deeplink: {}",
                deeplink.original()
            );
            if let Err(e) = StartupDeeplinkStorage::write(deeplink.original()) {
                log::error!("Cannot write startup deeplink: {e}");
            }
        }
    }

    if AuthTokenStorage::has_token() {
        log::info!("Token already installed");
        return Ok(());
    }

    let token = origin
        .and_then(|o| o.auth_token)
        .ok_or_else(|| anyhow!("Token not found in Zone.Identifier download-origin metadata"))?;
    AuthTokenStorage::write_token(token.as_str())?;
    log::info!("Token write complete");
    Ok(())
}

fn read_zone_origin(installer_path: &str) -> Result<DownloadOriginData> {
    let content = zone_identifier_content(installer_path)
        .or_else(|e| {
            log::error!("ADS read from direct CAPI failed, fallback to PowerShell: {e:?}");
            zone_identifier_content_powershell(installer_path)
        })
        .with_context(|| {
            anyhow!(
                "Reading zone content from both CAPI and PowerShell failed for '{installer_path}'"
            )
        })?;

    Ok(origin_from_zone_info(parsed_zone_identifier(&content)))
}

/// Merge the download-origin data carried by the Zone.Identifier URLs, taking
/// the first non-empty value for each field (host URL before referrer).
fn origin_from_zone_info(zone_info: ZoneInfo) -> DownloadOriginData {
    let mut result = DownloadOriginData::default();

    for url in [
        zone_info.host_url.as_deref(),
        zone_info.referrer_url.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(parsed) = DownloadOriginData::from_url(url) {
            result.auth_token = result.auth_token.or(parsed.auth_token);
            result.campaign_anon_user_id = result
                .campaign_anon_user_id
                .or(parsed.campaign_anon_user_id);
            result.startup_position = result.startup_position.or(parsed.startup_position);
            result.startup_realm = result.startup_realm.or(parsed.startup_realm);
            result.referrer = result.referrer.or(parsed.referrer);
            result.dcl_env = result.dcl_env.or(parsed.dcl_env);
        }
    }

    result
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

    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-Command")
        .arg(format!(
            "Get-Content -Path '{}' -Stream Zone.Identifier",
            path.replace("'", "''") // escape single quotes
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Without this the console-subsystem child gets a brand new console window,
    // since this binary runs windowless and has no console to inherit.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output()?;

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

        let origin = read_zone_origin(path)?;
        let token = origin.auth_token.ok_or_else(|| anyhow!("No token found"))?;
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

        let token = origin_from_zone_info(zone)
            .auth_token
            .ok_or_else(|| anyhow!("Token not found"))?;
        assert_eq!(expected_token, token.as_str());
        Ok(())
    }

    #[rstest]
    // HostUrl carries the environment.
    #[case(
        Some("https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/Decentraland_installer.exe"),
        None,
        Some("zone")
    )]
    #[case(
        Some("https://download-gateway.decentraland.org/391a85da-a3bb-49e2-a45e-96c740c38424/Decentraland_installer.exe"),
        None,
        Some("org")
    )]
    // HostUrl is a CDN outside decentraland.*: the referring page still names
    // the environment the user downloaded from.
    #[case(
        Some("https://cdn.example.com/Decentraland_installer.exe"),
        Some("https://decentraland.zone/download"),
        Some("zone")
    )]
    // Neither URL is a decentraland domain: no environment signal.
    #[case(
        Some("https://cdn.example.com/Decentraland_installer.exe"),
        Some("https://example.com/download"),
        None
    )]
    // No Zone.Identifier URLs at all (stripped ADS).
    #[case(None, None, None)]
    fn test_dcl_env_from_zone_info(
        #[case] host_url: Option<&str>,
        #[case] referrer_url: Option<&str>,
        #[case] expected: Option<&str>,
    ) {
        let zone = ZoneInfo {
            zone_id: Some(3),
            host_url: host_url.map(ToOwned::to_owned),
            referrer_url: referrer_url.map(ToOwned::to_owned),
        };

        let dcl_env = origin_from_zone_info(zone).dcl_env;
        assert_eq!(expected, dcl_env.map(|env| env.as_str()));
    }
}
