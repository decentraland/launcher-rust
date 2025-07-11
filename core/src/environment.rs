use clap::ArgAction;
use log::info;

use crate::config;

const DEFAULT_PROVIDER: &str = "dcl";

const BUCKET_URL: &str = env!("VITE_AWS_S3_BUCKET_PUBLIC_URL");
const PROVIDER: Option<&str> = option_env!("VITE_PROVIDER");
const LAUNCHER_ENVIRONMENT: Option<&str> = option_env!("LAUNCHER_ENVIRONMENT");

const ARG_SKIP_ANALYTICS: &str = "skip-analytics";
const ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE: &str = "open-deeplink-in-new-instance";
const ARG_ALWAYS_TRIGGER_UPDATER: &str = "always-trigger-updater";
const ARG_NEVER_TRIGGER_UPDATER: &str = "never-trigger-updater";
const ARG_USE_UPDATER_URL: &str = "use-updater-url";

const ARGS_KNOWN: &[&str] = &[
    ARG_SKIP_ANALYTICS,
    ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE,
    ARG_ALWAYS_TRIGGER_UPDATER,
    ARG_NEVER_TRIGGER_UPDATER,
    ARG_USE_UPDATER_URL,
];

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
            use_updater_url: self
                .use_updater_url
                .clone()
                .or_else(|| other.use_updater_url.clone()),
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

    Command::new(env!("CARGO_PKG_NAME"))
        .arg(
            Arg::new(ARG_SKIP_ANALYTICS)
                .long(ARG_SKIP_ANALYTICS)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE)
                .long(ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(ARG_ALWAYS_TRIGGER_UPDATER)
                .long(ARG_ALWAYS_TRIGGER_UPDATER)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(ARG_NEVER_TRIGGER_UPDATER)
                .long(ARG_NEVER_TRIGGER_UPDATER)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(ARG_USE_UPDATER_URL)
                .long(ARG_USE_UPDATER_URL)
                .value_name("URL")
                .num_args(0..=1),
        ) // optional
        .allow_external_subcommands(true)
}

fn filter_known_args(args: impl Iterator<Item = String>) -> impl Iterator<Item = String> {
    args.filter(|e| {
        if e.starts_with("--") {
            let without_dashes = e.trim_start_matches("--");
            ARGS_KNOWN.contains(&without_dashes)
        } else {
            // ignore none flags
            true
        }
    })
}

fn parse(i: impl Iterator<Item = String>) -> Result<Args, clap::Error> {
    let args = filter_known_args(i);
    let matches = build_command().try_get_matches_from(args)?;
    Ok(Args {
        skip_analytics: matches
            .get_one::<bool>(ARG_SKIP_ANALYTICS)
            .copied()
            .unwrap_or(false),
        open_deeplink_in_new_instance: matches
            .get_one::<bool>(ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE)
            .copied()
            .unwrap_or(false),
        always_trigger_updater: matches
            .get_one::<bool>(ARG_ALWAYS_TRIGGER_UPDATER)
            .copied()
            .unwrap_or(false),
        never_trigger_updater: matches
            .get_one::<bool>(ARG_NEVER_TRIGGER_UPDATER)
            .copied()
            .unwrap_or(false),
        use_updater_url: matches.get_one::<String>(ARG_USE_UPDATER_URL).cloned(),
    })
}
#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::*;

    #[test]
    fn test_known_args_parsed() -> Result<()> {
        let args = parse(
            [
                "app",
                "--skip-analytics",
                "--open-deeplink-in-new-instance",
                "--use-updater-url",
                "https://example.com",
            ]
            .map(ToOwned::to_owned)
            .into_iter(),
        )?;

        assert!(args.skip_analytics);
        assert!(args.open_deeplink_in_new_instance);
        assert!(!args.always_trigger_updater);
        assert_eq!(args.use_updater_url.as_deref(), Some("https://example.com"));
        Ok(())
    }

    #[test]
    fn test_unknown_args_ignored() -> Result<()> {
        let args = parse(
            [
                "app",
                "--skip-analytics",
                "--unknown-flag",
                "--use-updater-url",
                "https://example.com",
            ]
            .map(ToOwned::to_owned)
            .into_iter(),
        )?;

        assert!(args.skip_analytics);
        assert!(args.use_updater_url.is_some());
        Ok(())
    }

    #[test]
    fn test_merge_with() {
        let a = Args {
            skip_analytics: true,
            open_deeplink_in_new_instance: false,
            always_trigger_updater: true,
            never_trigger_updater: false,
            use_updater_url: Some("https://one.com".into()),
        };

        let b = Args {
            skip_analytics: false,
            open_deeplink_in_new_instance: true,
            always_trigger_updater: false,
            never_trigger_updater: true,
            use_updater_url: Some("https://two.com".into()),
        };

        let merged = a.merge_with(&b);

        assert!(merged.skip_analytics);
        assert!(merged.open_deeplink_in_new_instance);
        assert!(merged.always_trigger_updater);
        assert!(merged.never_trigger_updater);
        // Should keep first if present
        assert_eq!(merged.use_updater_url.as_deref(), Some("https://one.com"));
    }
}
