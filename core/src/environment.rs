pub mod macos;

use log::info;

use crate::config;

const DEFAULT_PROVIDER: &str = "dcl";

const BUCKET_URL: &str = env!("VITE_AWS_S3_BUCKET_PUBLIC_URL");
const PROVIDER: Option<&str> = option_env!("VITE_PROVIDER");
const LAUNCHER_ENVIRONMENT: Option<&str> = option_env!("LAUNCHER_ENVIRONMENT");

const ARG_SKIP_ANALYTICS: &str = "skip-analytics";
const ARG_FORCE_IN_MEMORY_ANALYTICS_QUEUE: &str = "force-in-memory-analytics-queue";
const ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE: &str = "open-deeplink-in-new-instance";
const ARG_ALWAYS_TRIGGER_UPDATER: &str = "always-trigger-updater";
const ARG_NEVER_TRIGGER_UPDATER: &str = "never-trigger-updater";
const ARG_USE_UPDATER_URL: &str = "use-updater-url";
const ARG_LOCAL_SCENE: &str = "local-scene";

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
    pub force_in_memory_analytics_queue: bool,
    pub open_deeplink_in_new_instance: bool,

    pub always_trigger_updater: bool,
    pub never_trigger_updater: bool,
    pub use_updater_url: Option<String>,

    // used by the client
    pub local_scene: bool,
}

impl Args {
    #[must_use]
    pub fn merge_with(&self, other: &Self) -> Self {
        Self {
            skip_analytics: self.skip_analytics || other.skip_analytics,
            force_in_memory_analytics_queue: self.force_in_memory_analytics_queue
                || other.force_in_memory_analytics_queue,
            open_deeplink_in_new_instance: self.open_deeplink_in_new_instance
                || other.open_deeplink_in_new_instance,
            always_trigger_updater: self.always_trigger_updater || other.always_trigger_updater,
            never_trigger_updater: self.never_trigger_updater || other.never_trigger_updater,
            use_updater_url: self
                .use_updater_url
                .clone()
                .or_else(|| other.use_updater_url.clone()),
            local_scene: self.local_scene || other.local_scene,
        }
    }

    pub fn parse(iterator: impl Iterator<Item = String>) -> Self {
        let vector: Vec<String> = iterator.collect();

        Self {
            skip_analytics: Self::has_flag(ARG_SKIP_ANALYTICS, &vector),
            force_in_memory_analytics_queue: Self::has_flag(
                ARG_FORCE_IN_MEMORY_ANALYTICS_QUEUE,
                &vector,
            ),
            open_deeplink_in_new_instance: Self::has_flag(
                ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE,
                &vector,
            ),
            always_trigger_updater: Self::has_flag(ARG_ALWAYS_TRIGGER_UPDATER, &vector),
            never_trigger_updater: Self::has_flag(ARG_NEVER_TRIGGER_UPDATER, &vector),
            use_updater_url: Self::value_by_flag(ARG_USE_UPDATER_URL, &vector),
            local_scene: Self::has_flag(ARG_LOCAL_SCENE, &vector),
        }
    }

    fn has_flag(flag: &str, i: &[String]) -> bool {
        i.iter().any(|e| {
            if e.starts_with("--") {
                let without_dashes = e.trim_start_matches("--");
                flag == without_dashes
            } else {
                false
            }
        })
    }

    fn value_by_flag(flag: &str, i: &[String]) -> Option<String> {
        let mut iter = i.iter().peekable();

        while let Some(arg) = iter.next() {
            if arg.trim_start_matches("--") == flag {
                if let Some(next) = iter.peek() {
                    if !next.starts_with("--") {
                        return Some(next.to_owned().to_owned());
                    }
                }
                return None;
            }
        }

        None
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

    pub fn cmd_args() -> Args {
        let (from_cmd, from_config) = Self::args_sources();
        let cmd_args = Args::parse(from_cmd);
        let config_args = Args::parse(from_config);
        let args = cmd_args.merge_with(&config_args);
        log::info!("parsed args: {:#?}", args);
        args
    }

    pub fn raw_cmd_args() -> impl Iterator<Item = String> {
        let (from_cmd, from_config) = Self::args_sources();
        from_cmd.chain(from_config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_args_parsed() {
        let args = Args::parse(
            [
                "app",
                "--skip-analytics",
                "--open-deeplink-in-new-instance",
                "--never-trigger-updater",
                "--use-updater-url",
                "https://example.com",
            ]
            .map(ToOwned::to_owned)
            .into_iter(),
        );

        assert!(args.skip_analytics);
        assert!(args.open_deeplink_in_new_instance);
        assert!(!args.always_trigger_updater);
        assert!(args.never_trigger_updater);
        assert_eq!(args.use_updater_url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn test_known_args_parsed_single_no_app_name() {
        let args = Args::parse(
            ["--never-trigger-updater"]
                .map(ToOwned::to_owned)
                .into_iter(),
        );

        assert!(!args.skip_analytics);
        assert!(!args.open_deeplink_in_new_instance);
        assert!(!args.always_trigger_updater);
        assert!(args.never_trigger_updater);
        assert!(args.use_updater_url.is_none());
    }

    #[test]
    fn test_unknown_args_ignored() {
        let args = Args::parse(
            [
                "app",
                "--skip-analytics",
                "--unknown-flag",
                "--use-updater-url",
                "https://example.com",
            ]
            .map(ToOwned::to_owned)
            .into_iter(),
        );

        assert!(args.skip_analytics);
        assert!(args.use_updater_url.is_some());
    }

    #[test]
    fn test_merge_with() {
        let a = Args {
            skip_analytics: true,
            force_in_memory_analytics_queue: false,
            open_deeplink_in_new_instance: false,
            always_trigger_updater: true,
            never_trigger_updater: false,
            use_updater_url: Some("https://one.com".into()),
            local_scene: false,
        };

        let b = Args {
            skip_analytics: false,
            force_in_memory_analytics_queue: false,
            open_deeplink_in_new_instance: true,
            always_trigger_updater: false,
            never_trigger_updater: true,
            use_updater_url: Some("https://two.com".into()),
            local_scene: false,
        };

        let merged = a.merge_with(&b);

        assert!(merged.skip_analytics);
        assert!(merged.open_deeplink_in_new_instance);
        assert!(merged.always_trigger_updater);
        assert!(merged.never_trigger_updater);
        assert!(!merged.local_scene);
        // Should keep first if present
        assert_eq!(merged.use_updater_url.as_deref(), Some("https://one.com"));
    }
}
