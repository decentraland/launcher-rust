use std::collections::HashMap;
use std::result::Result;
use std::sync::Mutex;
use url::form_urlencoded;

use log::{error, info, warn};

static PROTOCOL_STATE: Mutex<Option<DeepLink>> = Mutex::new(None);
const PROTOCOL_PREFIX: &str = "decentraland://";

#[derive(Default, Clone)]
pub struct Protocol {}

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

    pub fn save_to_file(deeplink: &DeepLink) {
        let path = crate::installs::deeplink_state_path();
        if let Err(e) = std::fs::write(&path, deeplink.original()) {
            error!("Failed to persist deep link to file: {}", e);
        } else {
            info!("Persisted deep link to {}", path.display());
        }
    }

    pub fn load_from_file() -> Option<String> {
        let path = crate::installs::deeplink_state_path();
        match std::fs::read_to_string(&path) {
            Ok(content) if !content.is_empty() => {
                info!("Loaded persisted deep link from {}", path.display());
                Some(content)
            }
            Ok(_) | Err(_) => None,
        }
    }

    pub fn clear_file() {
        let path = crate::installs::deeplink_state_path();
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                error!("Failed to remove persisted deep link file: {}", e);
            } else {
                info!("Cleared persisted deep link file");
            }
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
}
