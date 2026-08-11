//! Compile-time build targets (bucket, provider, launcher environment) and
//! the command-line arguments the launcher understands.
//!
//! Every process parses the same flag set, so the definitions live here. The
//! thin UI reads argv only ([`Args::from_argv`]) — the service merges
//! `config.json` arguments on top of argv with [`AppEnvironment::cmd_args`].

use log::info;

use crate::config;

const DEFAULT_PROVIDER: &str = "dcl";

const BUCKET_URL: &str = env!("VITE_AWS_S3_BUCKET_PUBLIC_URL");
const PROVIDER: Option<&str> = option_env!("VITE_PROVIDER");
const LAUNCHER_ENVIRONMENT: Option<&str> = option_env!("LAUNCHER_ENVIRONMENT");

const ARG_SKIP_ANALYTICS: &str = "skip-analytics";
const ARG_FORCE_IN_MEMORY_ANALYTICS_QUEUE: &str = "force-in-memory-analytics-queue";
const ARG_ALWAYS_TRIGGER_UPDATER: &str = "always-trigger-updater";
const ARG_NEVER_TRIGGER_UPDATER: &str = "never-trigger-updater";
const ARG_USE_UPDATER_URL: &str = "use-updater-url";

const ARG_USE_LATEST_JSON_URL: &str = "use-latest-json-url";

pub const ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE: &str = "open-deeplink-in-new-instance";
// Alias of ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE: either flag enables the same behavior.
pub const ARG_MULTI_INSTANCE: &str = "multi-instance";
pub const ARG_LOCAL_SCENE: &str = "local-scene";
/// When present, run in bridge-only mode (no client UI).
/// Used for deeplink/file-bridge integrations and headless operations.
pub const ARG_BRIDGE_ONLY: &str = "bridgeOnly";

/// The protocol every launcher deeplink starts with.
pub const DEEPLINK_PREFIX: &str = "decentraland://";

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
    pub open_new_client_instance: bool,

    pub always_trigger_updater: bool,
    pub never_trigger_updater: bool,
    pub use_updater_url: Option<String>,

    pub use_latest_json_url: Option<String>,

    // used by the client
    pub local_scene: bool,
    pub bridge_only: bool,
}

impl Args {
    #[must_use]
    pub fn merge_with(&self, other: &Self) -> Self {
        Self {
            skip_analytics: self.skip_analytics || other.skip_analytics,
            force_in_memory_analytics_queue: self.force_in_memory_analytics_queue
                || other.force_in_memory_analytics_queue,
            open_new_client_instance: self.open_new_client_instance
                || other.open_new_client_instance,
            always_trigger_updater: self.always_trigger_updater || other.always_trigger_updater,
            never_trigger_updater: self.never_trigger_updater || other.never_trigger_updater,
            use_updater_url: self
                .use_updater_url
                .clone()
                .or_else(|| other.use_updater_url.clone()),
            use_latest_json_url: self
                .use_latest_json_url
                .clone()
                .or_else(|| other.use_latest_json_url.clone()),
            local_scene: self.local_scene || other.local_scene,
            bridge_only: self.bridge_only || other.bridge_only,
        }
    }

    /// argv only, with no `config.json` merge — what the thin UI parses for
    /// itself: the updater runs in the UI process, everything else in argv is
    /// forwarded verbatim to the service, which merges config arguments on its
    /// own.
    pub fn from_argv() -> Self {
        Self::parse(std::env::args())
    }

    pub fn parse(iterator: impl Iterator<Item = String>) -> Self {
        let vector: Vec<String> = iterator.collect();

        Self {
            skip_analytics: Self::has_flag(ARG_SKIP_ANALYTICS, &vector),
            force_in_memory_analytics_queue: Self::has_flag(
                ARG_FORCE_IN_MEMORY_ANALYTICS_QUEUE,
                &vector,
            ),
            open_new_client_instance: Self::has_flag(
                ARG_OPEN_DEEPLINK_IN_NEW_INSTANCE,
                &vector,
            ) || Self::has_flag(ARG_MULTI_INSTANCE, &vector),
            always_trigger_updater: Self::has_flag(ARG_ALWAYS_TRIGGER_UPDATER, &vector),
            never_trigger_updater: Self::has_flag(ARG_NEVER_TRIGGER_UPDATER, &vector),
            use_updater_url: Self::value_by_flag(ARG_USE_UPDATER_URL, &vector),
            use_latest_json_url: Self::value_by_flag(ARG_USE_LATEST_JSON_URL, &vector),
            local_scene: Self::has_flag(ARG_LOCAL_SCENE, &vector),
            bridge_only: Self::has_flag(ARG_BRIDGE_ONLY, &vector),
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
        // Test-only escape hatch:  Debug builds only.
        #[cfg(debug_assertions)]
        if let Ok(url) = std::env::var("DCL_LAUNCHER_BUCKET_URL") {
            if !url.is_empty() {
                return url;
            }
        }
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

    /// argv merged with the `cmd-arguments` entry of `config.json`.
    pub fn cmd_args() -> Args {
        let from_cmd = std::env::args();
        info!("cmd args: {:?}", from_cmd);
        let from_config = config::cmd_arguments();
        info!("config args: {:?}", from_config);

        let args = Args::parse(from_cmd).merge_with(&Args::parse(from_config.into_iter()));
        info!("parsed args: {:#?}", args);
        args
    }
}

pub fn deeplink_from_env() -> Option<String> {
    std::env::args().find(|arg| is_deeplink(arg))
}

pub fn is_deeplink(value: &str) -> bool {
    value.starts_with(DEEPLINK_PREFIX)
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
        assert!(args.open_new_client_instance);
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
        assert!(!args.open_new_client_instance);
        assert!(!args.always_trigger_updater);
        assert!(args.never_trigger_updater);
        assert!(args.use_updater_url.is_none());
    }

    #[test]
    fn test_multi_instance_is_alias_of_open_deeplink_in_new_instance() {
        let args = Args::parse(["app", "--multi-instance"].map(ToOwned::to_owned).into_iter());
        assert!(args.open_new_client_instance);

        // The original flag still works on its own.
        let args = Args::parse(
            ["app", "--open-deeplink-in-new-instance"]
                .map(ToOwned::to_owned)
                .into_iter(),
        );
        assert!(args.open_new_client_instance);

        // Neither flag => false.
        let args = Args::parse(["app"].map(ToOwned::to_owned).into_iter());
        assert!(!args.open_new_client_instance);
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
            open_new_client_instance: false,
            always_trigger_updater: true,
            never_trigger_updater: false,
            use_updater_url: Some("https://one.com".into()),
            use_latest_json_url: None,
            local_scene: false,
            bridge_only: false,
        };

        let b = Args {
            skip_analytics: false,
            force_in_memory_analytics_queue: false,
            open_new_client_instance: true,
            always_trigger_updater: false,
            never_trigger_updater: true,
            use_updater_url: Some("https://two.com".into()),
            use_latest_json_url: Some("https://one.com".into()),
            local_scene: false,
            bridge_only: false,
        };

        let merged = a.merge_with(&b);

        assert!(merged.skip_analytics);
        assert!(merged.open_new_client_instance);
        assert!(merged.always_trigger_updater);
        assert!(merged.never_trigger_updater);
        assert!(!merged.local_scene);
        // Should keep first if present
        assert_eq!(merged.use_updater_url.as_deref(), Some("https://one.com"));
        assert_eq!(
            merged.use_latest_json_url.as_deref(),
            Some("https://one.com")
        );
    }

    #[test]
    fn deeplink_detection_requires_protocol_prefix() {
        assert!(is_deeplink("decentraland://open?position=0,0"));
        assert!(!is_deeplink("--skip-analytics"));
        assert!(!is_deeplink("https://decentraland.org"));
    }
}
