use std::collections::HashMap;
use std::result::Result;
use std::sync::Mutex;
use url::form_urlencoded;

use log::{error, warn};

static PROTOCOL_STATE: Mutex<Option<DeepLink>> = Mutex::new(None);
const PROTOCOL_PREFIX: &str = "decentraland://";

#[derive(Default, Clone)]
pub struct Protocol {}

#[derive(Debug)]
pub enum DeepLinkCreateError {
    WrongPrefix { original_content: String },
}

#[derive(Clone)]
pub struct DeepLink {
    original: String,
    args: HashMap<String, String>,
}

impl DeepLink {
    fn new(value: String) -> Result<Self, DeepLinkCreateError> {
        if value.starts_with(PROTOCOL_PREFIX) {
            let args = Self::parsed_args(value.as_str());
            let result = Self {
                original: value,
                args,
            };
            Ok(result)
        } else {
            Err(DeepLinkCreateError::WrongPrefix {
                original_content: value,
            })
        }
    }

    fn parsed_args(value: &str) -> HashMap<String, String> {
        let parts: Vec<&str> = value.splitn(2, "://").collect();

        match parts.get(1) {
            Some(query) => {
                let scheme = parts.first().unwrap_or(&"unknown");
                log::info!("Deeplink scheme: {}", scheme);

                let parsed = form_urlencoded::parse(query.as_bytes());
                let mut map: HashMap<String, String> = HashMap::new();

                for (key, value) in parsed {
                    map.insert(key.into_owned(), value.into_owned());
                }

                map
            }
            None => {
                log::info!("Cannot get query from: {}", value);
                HashMap::new()
            }
        }
    }

    pub fn has_true_value(&self, key: &str) -> bool {
        if let Some(value) = self.args.get(key) {
            value == "true"
        } else {
            false
        }
    }

    pub fn original(&self) -> &str {
        &self.original
    }

    #[cfg(test)]
    pub(crate) fn from_string(value: impl Into<String>) -> Result<Self, DeepLinkCreateError> {
        Self::new(value.into())
    }
}

impl From<DeepLink> for String {
    fn from(deeplink: DeepLink) -> Self {
        deeplink.original
    }
}

impl Protocol {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    pub fn value() -> Option<DeepLink> {
        let result = PROTOCOL_STATE.lock();
        match result {
            Ok(guard) => guard.clone(),
            Err(e) => {
                error!("cannot acquire mutex of PROTOCOL_STATE: {}", e);
                None
            }
        }
    }

    pub fn try_assign_value(&self, value: String) {
        match DeepLink::new(value) {
            Ok(deeplink) => {
                let result = PROTOCOL_STATE.lock();
                match result {
                    Ok(guard) => {
                        let mut guard = guard;
                        *guard = Some(deeplink);
                    }
                    Err(e) => {
                        error!("cannot acquire mutex of PROTOCOL_STATE: {}", e);
                    }
                }
            }
            Err(error) => match error {
                DeepLinkCreateError::WrongPrefix { original_content } => {
                    error!(
                        "trying assing value that doesn't start with prefix protocol {}: {}",
                        PROTOCOL_PREFIX, original_content
                    );
                }
            },
        }
    }

    pub fn try_assign_value_from_vec(&self, value: &Vec<String>) {
        for v in value {
            if let Ok(deeplink) = DeepLink::new(v.to_owned()) {
                self.try_assign_value(deeplink.into());
                return;
            }
        }

        warn!(
            "none of values starts with prefix protocol {}: {:?}",
            PROTOCOL_PREFIX, value
        );
    }

    /// If no URI-scheme deeplink was received, seed Protocol state from the
    /// startup location (`startup-location.txt`) written during the
    /// download/install flow. Called once in `lib.rs` after `setup_deeplink()`.
    pub fn try_seed_from_startup_location() {
        if Self::value().is_some() {
            log::info!("URI deeplink already set; skipping startup location seed");
            return;
        }
        match crate::download_origin_metadata::startup_location_storage::StartupLocationStorage::consume() {
            Some(deeplink) => {
                log::info!("Seeding Protocol from startup location: {}", deeplink);
                let p = Self::new();
                p.try_assign_value(deeplink);
            }
            None => log::info!("No startup location found"),
        }
    }
}
