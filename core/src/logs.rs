use crate::installs;
use anyhow::Result;
use log::{Metadata, Record, info};
use sentry_log::SentryLogger;

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
