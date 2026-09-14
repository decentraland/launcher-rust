use log::{error, info, LevelFilter};

/// The UI logs to its own `output-ui.log` — the service (via core) owns
/// `output.log`, and two processes appending one file interleave.
pub fn init() {
    let mut dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                humantime::format_rfc3339_seconds(std::time::SystemTime::now()),
                record.level(),
                record.target(),
                message
            ));
        })
        .level(LevelFilter::Info)
        .chain(std::io::stdout());

    match dcl_launcher_shared::ui_log_file_path() {
        Ok(path) => match fern::log_file(&path) {
            Ok(file) => {
                dispatch = dispatch.chain(file);
            }
            Err(e) => eprintln!("Cannot open the UI log file {}: {e}", path.display()),
        },
        Err(e) => eprintln!("Cannot resolve the UI log file path: {e:#}"),
    }

    if let Err(e) = dispatch.apply() {
        eprintln!("Cannot initialize UI logging: {e}");
    }

    std::panic::set_hook(Box::new(|panic_info| {
        error!("Panic occurred: {:?}", panic_info);
    }));

    init_sentry();
}

fn init_sentry() {
    let Some(dsn_str) = option_env!("SENTRY_DSN") else {
        info!("SENTRY_DSN is not provided, Sentry is disabled for the UI");
        return;
    };

    let Ok(dsn) = dsn_str.parse() else {
        error!("Cannot parse the provided SENTRY_DSN, Sentry is disabled for the UI");
        return;
    };

    let environment = option_env!("LAUNCHER_ENVIRONMENT").unwrap_or("unknown");
    let release = format!("launcher-ui@{}", dcl_launcher_shared::app_version());

    let options = sentry::ClientOptions {
        dsn: Some(dsn),
        release: Some(release.into()),
        environment: Some(environment.to_owned().into()),
        attach_stacktrace: true,
        // The service (via core) owns session tracking; a second session per
        // launch would double-count.
        auto_session_tracking: false,
        ..Default::default()
    };

    let guard = sentry::init(options);
    std::mem::forget(guard);
    info!("Sentry is initialized for the UI");
}
