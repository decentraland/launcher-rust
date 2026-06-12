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

    /// Returns the pending deep link and clears its persisted file so it
    /// cannot be re-consumed by a future launch. Falls back to the file when
    /// the in-memory state is empty (e.g. after a self-update restart).
    pub fn consume_deeplink() -> Option<DeepLink> {
        let deeplink = Self::value().or_else(|| {
            let deeplink = Self::try_load_from_file()?;
            // Keep it in memory so a retried launch still sees the deep link
            Self::try_store_in_memory(deeplink.clone());
            Some(deeplink)
        });

        if deeplink.is_some() {
            Self::try_clear_file();
        }

        deeplink
    }

    pub fn try_assign_value(&self, value: String) {
        match DeepLink::new(value) {
            Ok(deeplink) => {
                // Persist so the deep link survives the self-update restart:
                // update.install() exits the process on Windows before the
                // in-memory state can be carried over
                Self::try_save_to_file(&deeplink);
                Self::try_store_in_memory(deeplink);
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

    fn try_store_in_memory(deeplink: DeepLink) {
        match PROTOCOL_STATE.lock() {
            Ok(mut guard) => {
                *guard = Some(deeplink);
            }
            Err(e) => {
                error!("cannot acquire mutex of PROTOCOL_STATE: {}", e);
            }
        }
    }

    fn try_save_to_file(deeplink: &DeepLink) {
        let path = crate::installs::deeplink_state_path();
        if let Err(e) = std::fs::write(&path, deeplink.original()) {
            error!("Failed to persist deep link to {}: {}", path.display(), e);
        } else {
            info!("Persisted deep link to {}", path.display());
        }
    }

    fn try_load_from_file() -> Option<DeepLink> {
        let path = crate::installs::deeplink_state_path();
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    error!(
                        "Failed to read persisted deep link from {}: {}",
                        path.display(),
                        e
                    );
                }
                return None;
            }
        };

        if content.is_empty() {
            warn!("Persisted deep link file {} is empty", path.display());
            return None;
        }

        match DeepLink::new(content) {
            Ok(deeplink) => {
                info!("Loaded persisted deep link from {}", path.display());
                Some(deeplink)
            }
            Err(DeepLinkCreateError::WrongPrefix { original_content }) => {
                error!(
                    "persisted deep link doesn't start with prefix protocol {}: {}",
                    PROTOCOL_PREFIX, original_content
                );
                None
            }
        }
    }

    fn try_clear_file() {
        let path = crate::installs::deeplink_state_path();
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                error!(
                    "Failed to remove persisted deep link file {}: {}",
                    path.display(),
                    e
                );
            }
        } else {
            info!("Cleared persisted deep link file");
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
