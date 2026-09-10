//! Log tails attached to the crash-report Sentry event when the user ticks
//! "Share diagnostic logs".

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use sentry::protocol::Attachment;

use crate::installs;

/// Same cap the Explorer applies to its own log attachment.
pub const LOG_TAIL_MAX_BYTES: usize = 64 * 1024;

const TEXT_PLAIN: &str = "text/plain";
const EXPLORER_LOG_FILE_NAME: &str = "Player.log";
const LAUNCHER_LOG_FILE_NAME: &str = "output.log";

/// Unity `Player.log` of the Explorer (`companyName: Decentraland`, `productName: Explorer`).
pub fn explorer_player_log_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    #[cfg(target_os = "macos")]
    let path = home.join("Library/Logs/Decentraland/Explorer/Player.log");

    #[cfg(target_os = "windows")]
    let path = home.join("AppData\\LocalLow\\Decentraland\\Explorer\\Player.log");

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let path = home.join(".config/unity3d/Decentraland/Explorer/Player.log");

    Some(path)
}

/// Both tails, skipping any file that is missing or unreadable.
pub fn collect() -> Vec<Attachment> {
    let mut attachments = Vec::with_capacity(2);

    if let Some(path) = explorer_player_log_path() {
        attachments.extend(attachment_for(&path, EXPLORER_LOG_FILE_NAME));
    }

    match installs::log_file_path() {
        Ok(path) => attachments.extend(attachment_for(&path, LAUNCHER_LOG_FILE_NAME)),
        Err(e) => log::warn!("Cannot resolve the launcher log path: {e}"),
    }

    attachments
}

fn attachment_for(path: &Path, filename: &str) -> Option<Attachment> {
    let buffer = tail(path, LOG_TAIL_MAX_BYTES)?;
    Some(Attachment {
        buffer,
        filename: filename.to_owned(),
        content_type: Some(TEXT_PLAIN.to_owned()),
        ty: None,
    })
}

/// Last `max_bytes` of `path`, or `None` when the file cannot be read.
pub fn tail(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) => {
            log::info!("Log {} not attached: {e}", path.display());
            return None;
        }
    };

    let len = file.metadata().ok()?.len();
    let max = u64::try_from(max_bytes).ok()?;
    let start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start)).ok()?;

    let mut buffer = Vec::with_capacity(usize::try_from(len.saturating_sub(start)).ok()?);
    file.read_to_end(&mut buffer).ok()?;
    Some(buffer)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_log(content: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("dcl-log-{}.log", uuid::Uuid::new_v4()));
        fs::write(&path, content).unwrap_or_else(|e| panic!("{e}"));
        path
    }

    #[test]
    fn tail_returns_whole_small_file() {
        let path = temp_log(b"hello");
        assert_eq!(tail(&path, 64).as_deref(), Some(&b"hello"[..]));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn tail_keeps_only_the_last_bytes() {
        let path = temp_log(b"0123456789");
        assert_eq!(tail(&path, 4).as_deref(), Some(&b"6789"[..]));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn tail_of_missing_file_is_none() {
        let path = std::env::temp_dir().join(format!("dcl-missing-{}.log", uuid::Uuid::new_v4()));
        assert!(tail(&path, 4).is_none());
    }

    #[test]
    fn attachment_is_text_plain_with_requested_name() {
        let path = temp_log(b"x");
        let attachment = attachment_for(&path, "Player.log").unwrap_or_else(|| panic!("none"));
        assert_eq!(attachment.filename, "Player.log");
        assert_eq!(attachment.content_type.as_deref(), Some(TEXT_PLAIN));
        let _ = fs::remove_file(path);
    }
}
