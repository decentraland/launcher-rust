use crate::installs;
use anyhow::Result;
use log::{Metadata, Record, info};
use sentry_log::SentryLogger;

pub fn dispath_logs() -> Result<()> {
    let path = installs::log_file_path()?;
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
                record.target(),
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

/// Opt-in log target for error logs that must NOT become standalone Sentry
/// events. Use it only on `log::error!` lines that duplicate an error already
/// reported via `sentry::capture_error` (which carries the grouping
/// fingerprint) — the log line then rides along as a breadcrumb instead of
/// creating a second, message-grouped issue.
pub const SENTRY_BREADCRUMB_ONLY_TARGET: &str = "sentry_breadcrumb_only";

fn new_sentry_log() -> SentryLogger<pretty_env_logger::env_logger::Logger> {
    // setup as in the guide: https://crates.io/crates/sentry-log
    let mut log_builder = pretty_env_logger::formatted_builder();
    log_builder.parse_filters("info");
    let log = log_builder.build();
    sentry_log::SentryLogger::with_dest(log).filter(breadcrumb_only_filter)
}

fn breadcrumb_only_filter(metadata: &Metadata) -> sentry_log::LogFilter {
    if metadata.target() == SENTRY_BREADCRUMB_ONLY_TARGET {
        sentry_log::LogFilter::Breadcrumb
    } else {
        sentry_log::default_filter(metadata)
    }
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
        self.fern.log(record);
        self.sentry.log(record);
    }

    fn flush(&self) {
        self.fern.flush();
        self.sentry.flush();
    }
}
