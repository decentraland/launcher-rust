use crate::installs;
use anyhow::Result;
use log::{Metadata, Record, info};
use sentry_log::SentryLogger;
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogDestination {
    File,
    Sentry,
    All,
}

impl LogDestination {
    const FILE_TARGET: &'static str = "dest:file";
    const SENTRY_TARGET: &'static str = "dest:sentry";
    const ALL_TARGET: &'static str = "dest:all";

    #[must_use]
    pub const fn as_target(self) -> &'static str {
        match self {
            Self::File => Self::FILE_TARGET,
            Self::Sentry => Self::SENTRY_TARGET,
            Self::All => Self::ALL_TARGET,
        }
    }
}

impl From<&str> for LogDestination {
    fn from(target: &str) -> Self {
        match target {
            Self::FILE_TARGET => Self::File,
            Self::SENTRY_TARGET => Self::Sentry,
            _ => Self::All,
        }
    }
}

pub fn dispath_logs() -> Result<()> {
    let path = installs::log_file_path()?;
    rotate_oversized_log(&path);
    let log_file = fern::log_file(&path)?;
    let path = path.to_string_lossy().to_string();
    println!("Write logs to path: {}", &path);

    let (level, fern_log) = fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                humantime::format_rfc3339(std::time::SystemTime::now()),
                record.level(),
                record.module_path().unwrap_or_else(|| record.target()),
                message
            ));
        })
        .level(log::LevelFilter::Trace)
        .chain(std::io::stdout())
        .chain(log_file)
        .into_log();

    let sentry_log = new_sentry_log();

    let log = CombinedLog {
        fern: fern_log,
        sentry: sentry_log,
    };

    log::set_boxed_logger(Box::new(log))?;
    log::set_max_level(level);

    info!("Logs setup to path: {}", &path);
    Ok(())
}

fn rotate_oversized_log(path: &Path) {
    const MAX_LOG_SIZE_BYTES: u64 = 50 * 1024 * 1024;

    let Ok(metadata) = fs::metadata(path) else {
        return;
    };

    if metadata.len() <= MAX_LOG_SIZE_BYTES {
        return;
    }

    let mut prev_path = path.as_os_str().to_owned();
    prev_path.push(".prev");
    if let Err(e) = fs::rename(path, &prev_path) {
        eprintln!("Failed to rotate log file {}: {}", path.display(), e);
    }
}

fn new_sentry_log() -> SentryLogger<pretty_env_logger::env_logger::Logger> {
    // setup as in the guide: https://crates.io/crates/sentry-log
    let mut log_builder = pretty_env_logger::formatted_builder();
    log_builder.parse_filters("info");
    let log = log_builder.build();
    sentry_log::SentryLogger::with_dest(log)
}

struct CombinedLog {
    fern: Box<dyn log::Log>,
    sentry: SentryLogger<pretty_env_logger::env_logger::Logger>,
}

impl log::Log for CombinedLog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.fern.enabled(metadata) && self.sentry.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        match LogDestination::from(record.target()) {
            LogDestination::File => self.fern.log(record),
            LogDestination::Sentry => self.sentry.log(record),
            LogDestination::All => {
                self.fern.log(record);
                self.sentry.log(record);
            }
        }
    }

    fn flush(&self) {
        self.fern.flush();
        self.sentry.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Mirrors the private, function-local `MAX_LOG_SIZE_BYTES` in
    // `rotate_oversized_log` above (50 MiB). That constant isn't reachable
    // from here (it's scoped to the function body, not the module), so this
    // copy has to be kept in sync with it by hand if the threshold ever
    // changes.
    const MAX_LOG_SIZE_BYTES: u64 = 50 * 1024 * 1024;
    const OVERSIZED_BYTES: u64 = MAX_LOG_SIZE_BYTES + 1;

    /// Panic-safe scratch directory under the OS temp dir. Every instance
    /// gets a name unique across parallel `cargo test` threads (pid + a
    /// per-process atomic counter + a timestamp) and is removed on drop, so
    /// a failing assertion still cleans up after itself.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> anyhow::Result<Self> {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "dcl-launcher-rotate-log-test-{label}-{}-{seq}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&dir)?;
            Ok(Self(dir))
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // (a) file over the threshold gets renamed to `.prev` and the original
    // path is gone. Built with `File::set_len` (a sparse file): on both
    // Unix (`ftruncate`) and Windows (`SetEndOfFile`) this sets the file's
    // logical size without writing real bytes, and `metadata().len()`
    // reports that logical size on both platforms, so the size check in
    // `rotate_oversized_log` sees it as a genuinely oversized file.
    #[test]
    fn oversized_file_is_renamed_to_prev_and_original_is_gone() -> anyhow::Result<()> {
        let dir = ScratchDir::new("oversized")?;
        let path = dir.join("output.log");
        fs::File::create(&path)?.set_len(OVERSIZED_BYTES)?;

        rotate_oversized_log(&path);

        assert!(!path.exists(), "original log should be gone after rotation");
        let prev_path = dir.join("output.log.prev");
        assert!(prev_path.exists(), ".prev file should exist after rotation");
        assert_eq!(fs::metadata(&prev_path)?.len(), OVERSIZED_BYTES);
        Ok(())
    }

    // (b) file under the threshold is left untouched, byte-for-byte.
    #[test]
    fn small_file_is_left_untouched() -> anyhow::Result<()> {
        let dir = ScratchDir::new("small")?;
        let path = dir.join("output.log");
        fs::write(&path, b"hello world")?;

        rotate_oversized_log(&path);

        assert!(path.exists(), "small log should still be present");
        assert_eq!(fs::read(&path)?, b"hello world");
        assert!(!dir.join("output.log.prev").exists());
        Ok(())
    }

    // (b, boundary) a file exactly at the threshold must not rotate either:
    // the helper's guard is `<=`, not `<`.
    #[test]
    fn file_exactly_at_threshold_is_left_untouched() -> anyhow::Result<()> {
        let dir = ScratchDir::new("boundary")?;
        let path = dir.join("output.log");
        fs::File::create(&path)?.set_len(MAX_LOG_SIZE_BYTES)?;

        rotate_oversized_log(&path);

        assert!(
            path.exists(),
            "a log exactly at the threshold must not rotate (comparison is <=, not <)"
        );
        assert_eq!(fs::metadata(&path)?.len(), MAX_LOG_SIZE_BYTES);
        Ok(())
    }

    // (c) a pre-existing `.prev` is replaced by the freshly-rotated file,
    // not appended to.
    #[test]
    fn existing_prev_file_is_replaced_not_appended_to() -> anyhow::Result<()> {
        let dir = ScratchDir::new("replace-prev")?;
        let path = dir.join("output.log");
        let prev_path = dir.join("output.log.prev");

        fs::write(&prev_path, b"stale previous run")?;
        fs::File::create(&path)?.set_len(OVERSIZED_BYTES)?;

        rotate_oversized_log(&path);

        assert!(!path.exists());
        assert!(prev_path.exists());
        assert_eq!(
            fs::metadata(&prev_path)?.len(),
            OVERSIZED_BYTES,
            "stale .prev should be replaced by the freshly-rotated file, not appended to"
        );
        Ok(())
    }

    // (d) missing source file: no-op, no panic.
    #[test]
    fn missing_file_is_a_silent_no_op() -> anyhow::Result<()> {
        let dir = ScratchDir::new("missing")?;
        let path = dir.join("does-not-exist.log");

        rotate_oversized_log(&path);

        assert!(!path.exists());
        assert!(!dir.join("does-not-exist.log.prev").exists());
        Ok(())
    }
}
