use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::installs::crash_reports_dir;

const FALLBACK_FILE_STEM: &str = "unknown-session";

/// What the crash watchdog knows about an unexpected Explorer exit.
///
/// The watchdog writes it as `<app dir>/crash-reports/<session_id>.json` and starts the launcher
/// with `--crash-report-with-attachment <path>`; the report flow reads it back and deletes it once
/// the report is submitted or dismissed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashAttachment {
    /// The `--session_id` the launcher passed to the Explorer; joins launcher, Explorer and Sentry.
    pub session_id: String,
    pub explorer_version: String,
    /// Human readable exit description, e.g. `exit 1`, `signal SIGSEGV`, `0xC0000005`.
    pub exit_code: String,
    /// Unix timestamp in seconds.
    pub crashed_at: u64,
    pub pid: u32,
}

impl CrashAttachment {
    pub fn default_path(&self) -> PathBuf {
        crash_reports_dir().join(format!("{}.json", file_stem_for(&self.session_id)))
    }

    /// Writes to [`Self::default_path`] and returns it.
    pub fn write(&self) -> Result<PathBuf> {
        let path = self.default_path();
        self.write_to(&path)?;
        Ok(path)
    }

    pub fn write_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("Cannot create {}", dir.display()))?;
        }
        let json = serde_json::to_string_pretty(self).context("Cannot serialize attachment")?;
        fs::write(path, json).with_context(|| format!("Cannot write {}", path.display()))?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Cannot read crash attachment {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Crash attachment {} is malformed", path.display()))
    }

    /// Best effort: a missing file is fine, anything else is logged.
    pub fn delete(path: &Path) {
        match fs::remove_file(path) {
            Ok(()) => log::info!("Crash attachment consumed: {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Cannot remove crash attachment {}: {e}", path.display()),
        }
    }
}

/// Session ids are UUIDs, but they arrive through argv, so anything that could escape the
/// crash-reports directory is stripped before the id becomes a file name.
fn file_stem_for(session_id: &str) -> String {
    let stem: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if stem.is_empty() {
        FALLBACK_FILE_STEM.to_owned()
    } else {
        stem
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    fn sample() -> CrashAttachment {
        CrashAttachment {
            session_id: "0f3c7b8e-1111-2222-3333-444455556666".to_owned(),
            explorer_version: "v1.2.3".to_owned(),
            exit_code: "signal SIGSEGV".to_owned(),
            crashed_at: 1_757_400_000,
            pid: 4242,
        }
    }

    fn temp_file() -> PathBuf {
        std::env::temp_dir()
            .join(format!("dcl-crash-attachment-{}", uuid::Uuid::new_v4()))
            .join("nested")
            .join("attachment.json")
    }

    #[test]
    fn write_read_round_trip_creates_missing_directories() {
        let path = temp_file();
        let attachment = sample();

        attachment
            .write_to(&path)
            .unwrap_or_else(|e| panic!("write failed: {e}"));
        let read = CrashAttachment::read(&path).unwrap_or_else(|e| panic!("read failed: {e}"));

        assert_eq!(read, attachment);
        CrashAttachment::delete(&path);
        assert!(!path.exists());
    }

    #[test]
    fn malformed_file_is_an_error_not_a_panic() {
        let path = temp_file();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).unwrap_or_else(|e| panic!("mkdir failed: {e}"));
        }
        fs::write(&path, "{ not json").unwrap_or_else(|e| panic!("write failed: {e}"));

        assert!(CrashAttachment::read(&path).is_err());
        CrashAttachment::delete(&path);
    }

    #[test]
    fn delete_of_missing_file_is_silent() {
        CrashAttachment::delete(&temp_file());
    }

    #[test]
    fn file_stem_strips_path_separators_and_falls_back_when_empty() {
        assert_eq!(file_stem_for("abc-123"), "abc-123");
        assert_eq!(file_stem_for("../../etc/passwd"), "etcpasswd");
        assert_eq!(file_stem_for("..\\..\\x"), "x");
        assert_eq!(file_stem_for("///"), FALLBACK_FILE_STEM);
    }
}
