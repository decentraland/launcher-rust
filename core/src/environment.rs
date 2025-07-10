use clap::Parser;
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
#[derive(clap::Parser, Debug)]
pub struct Args {
    #[arg(long)]
    pub skip_analytics: bool,
    #[arg(long)]
    pub open_deeplink_in_new_instance: bool,

    #[arg(long)]
    pub always_trigger_updater: bool,
    #[arg(long)]
    pub never_trigger_updater: bool,
    #[arg(long)]
    pub use_updater_url: Option<String>,
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

    pub fn cmd_args() -> Args {
        let raw = Self::raw_cmd_args();
        Args::parse_from(raw)
    }

    pub fn raw_cmd_args() -> impl Iterator<Item = String> {
        let from_cmd = std::env::args();
        info!("cmd args: {:?}", from_cmd);

        let from_config = config::cmd_arguments();
        info!("config args: {:?}", from_config);

        from_cmd.chain(from_config)
    }
}
