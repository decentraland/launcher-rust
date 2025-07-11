use log::info;

use crate::config;

const DEFAULT_PROVIDER: &str = "dcl";

const BUCKET_URL: &str = env!("VITE_AWS_S3_BUCKET_PUBLIC_URL");
const PROVIDER: Option<&str> = option_env!("VITE_PROVIDER");
const LAUNCHER_ENVIRONMENT: Option<&str> = option_env!("LAUNCHER_ENVIRONMENT");

#[derive(Debug)]
pub enum LauncherEnvironment {
    Production,
    Development,
    Unknown,
}

pub struct AppEnvironment {}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
pub struct Args {
    pub skip_analytics: bool,
    pub open_deeplink_in_new_instance: bool,

    pub always_trigger_updater: bool,
    pub never_trigger_updater: bool,
    pub use_updater_url: Option<String>,
}

impl Args {
    #[must_use]
    pub fn merge_with(&self, other: &Self) -> Self {
        Self {
            skip_analytics: self.skip_analytics || other.skip_analytics,
            open_deeplink_in_new_instance: self.open_deeplink_in_new_instance
                || other.open_deeplink_in_new_instance,
            always_trigger_updater: self.always_trigger_updater || other.always_trigger_updater,
            never_trigger_updater: self.never_trigger_updater || other.never_trigger_updater,
            use_updater_url: self.use_updater_url.clone().or_else(|| other.use_updater_url.clone()),
        }
    }
}

impl AppEnvironment {
    pub fn provider() -> String {
        let provider = PROVIDER.unwrap_or(DEFAULT_PROVIDER);
        String::from(provider)
    }

    pub fn bucket_url() -> String {
        String::from(BUCKET_URL)
    }

    pub fn launcher_environment() -> LauncherEnvironment {
        match LAUNCHER_ENVIRONMENT {
            Some(raw) => match raw {
                "prod" => LauncherEnvironment::Production,
                "dev" => LauncherEnvironment::Development,
                _ => LauncherEnvironment::Unknown,
            },
            None => LauncherEnvironment::Unknown,
        }
    }

    fn args_sources() -> (impl Iterator<Item = String>, impl Iterator<Item = String>) {
        let from_cmd = std::env::args();
        info!("cmd args: {:?}", from_cmd);
        let from_config = config::cmd_arguments();
        info!("config args: {:?}", from_config);

        (from_cmd, from_config.into_iter())
    }

    fn cmd_args_internal() -> Result<Args, clap::Error> {
        let (from_cmd, from_config) = Self::args_sources();
        let cmd_args = parse(from_cmd)?;
        let config_args = parse(from_config)?;

        let args = cmd_args.merge_with(&config_args);

        Ok(args)
    }

    pub fn cmd_args() -> Args {
        let args = Self::cmd_args_internal();
        match args {
            Ok(args) => {
                log::info!("parsed args: {:#?}", args);
                args
            }
            Err(e) => {
                log::error!("cannot pass args, fallback to default args: {}", e);
                Args::default()
            }
        }
    }

    pub fn raw_cmd_args() -> impl Iterator<Item = String> {
        let (from_cmd, from_config) = Self::args_sources();
        from_cmd.chain(from_config)
    }
}

// Allow external arguments
fn build_command() -> clap::Command {
    use clap::Arg;
    use clap::Command;

    Command::new("dcl_launcher")
        .arg(Arg::new("skip_analytics").long("skip-analytics"))
        .arg(Arg::new("open_deeplink_in_new_instance").long("open-deeplink-in-new-instance"))
        .arg(Arg::new("always_trigger_updater").long("always-trigger-updater"))
        .arg(Arg::new("never_trigger_updater").long("never-trigger-updater"))
        .arg(Arg::new("use_updater_url").long("use-updater-url"))
        .allow_external_subcommands(true)
}

fn parse(i: impl Iterator<Item = String>) -> Result<Args, clap::Error> {
    let matches = build_command().try_get_matches_from(i)?;
    Ok(Args {
        skip_analytics: matches.contains_id("skip_analytics"),
        open_deeplink_in_new_instance: matches.contains_id("open_deeplink_in_new_instance"),
        always_trigger_updater: matches.contains_id("always_trigger_updater"),
        never_trigger_updater: matches.contains_id("never_trigger_updater"),
        use_updater_url: matches.get_one::<String>("use_updater_url").cloned(),
    })
}
