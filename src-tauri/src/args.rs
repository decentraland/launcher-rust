const ARG_ALWAYS_TRIGGER_UPDATER: &str = "always-trigger-updater";
const ARG_NEVER_TRIGGER_UPDATER: &str = "never-trigger-updater";
const ARG_USE_UPDATER_URL: &str = "use-updater-url";

const DEEPLINK_PREFIX: &str = "decentraland://";

/// The only flags the thin UI parses itself: the updater runs in this
/// process. Everything else in argv is forwarded verbatim to the service
/// (which merges `config.json` arguments on its own).
#[derive(Debug, Default, Clone)]
pub struct UpdaterArgs {
    pub always_trigger_updater: bool,
    pub never_trigger_updater: bool,
    pub use_updater_url: Option<String>,
}

impl UpdaterArgs {
    pub fn parse_from_env() -> Self {
        Self::parse(std::env::args())
    }

    fn parse(iterator: impl Iterator<Item = String>) -> Self {
        let vector: Vec<String> = iterator.collect();
        Self {
            always_trigger_updater: has_flag(ARG_ALWAYS_TRIGGER_UPDATER, &vector),
            never_trigger_updater: has_flag(ARG_NEVER_TRIGGER_UPDATER, &vector),
            use_updater_url: value_by_flag(ARG_USE_UPDATER_URL, &vector),
        }
    }
}

pub fn deeplink_from_env() -> Option<String> {
    std::env::args().find(|arg| is_deeplink(arg))
}

pub fn is_deeplink(value: &str) -> bool {
    value.starts_with(DEEPLINK_PREFIX)
}

fn has_flag(flag: &str, args: &[String]) -> bool {
    args.iter().any(|e| {
        if e.starts_with("--") {
            let without_dashes = e.trim_start_matches("--");
            flag == without_dashes
        } else {
            false
        }
    })
}

fn value_by_flag(flag: &str, args: &[String]) -> Option<String> {
    let mut iter = args.iter().peekable();

    while let Some(arg) = iter.next() {
        if arg.trim_start_matches("--") == flag {
            if let Some(next) = iter.peek() {
                if !next.starts_with("--") {
                    return Some((*next).clone());
                }
            }
            return None;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_flags_are_parsed() {
        let args = UpdaterArgs::parse(
            [
                "app",
                "--never-trigger-updater",
                "--use-updater-url",
                "https://example.com",
            ]
            .map(ToOwned::to_owned)
            .into_iter(),
        );

        assert!(!args.always_trigger_updater);
        assert!(args.never_trigger_updater);
        assert_eq!(args.use_updater_url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn missing_flags_default_to_off() {
        let args = UpdaterArgs::parse(["app"].map(ToOwned::to_owned).into_iter());

        assert!(!args.always_trigger_updater);
        assert!(!args.never_trigger_updater);
        assert!(args.use_updater_url.is_none());
    }

    #[test]
    fn deeplink_detection_requires_protocol_prefix() {
        assert!(is_deeplink("decentraland://open?position=0,0"));
        assert!(!is_deeplink("--skip-analytics"));
        assert!(!is_deeplink("https://decentraland.org"));
    }
}
