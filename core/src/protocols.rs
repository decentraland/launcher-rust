use std::collections::HashMap;
use std::result::Result;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::form_urlencoded;

use log::{error, info, warn};

static PROTOCOL_STATE: Mutex<Option<DeepLink>> = Mutex::new(None);
const PROTOCOL_PREFIX: &str = "decentraland://";

/// How long a persisted deep link stays valid.
///
/// It only has to survive the self-update download + install + restart;
/// anything older is a leftover from a crashed or abandoned session and
/// must not replay into a new one.
const DEEPLINK_STATE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedDeepLink {
    url: String,
    saved_at_unix_secs: u64,
}

impl PersistedDeepLink {
    fn now_unix_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default()
    }

    fn new(deeplink: &DeepLink) -> Self {
        Self {
            url: deeplink.original().to_owned(),
            saved_at_unix_secs: Self::now_unix_secs(),
        }
    }

    fn is_expired(&self) -> bool {
        let age_secs = Self::now_unix_secs().saturating_sub(self.saved_at_unix_secs);
        age_secs > DEEPLINK_STATE_TTL.as_secs()
    }
}

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

    /// Returns the pending deep link.
    ///
    /// Falls back to the persisted file when the in-memory state is empty
    /// (e.g. after a self-update restart). The file is kept until
    /// [`Self::clear_persisted_deeplink`] confirms the launch delivered it,
    /// so a failed launch followed by another restart can still recover the
    /// deep link.
    pub fn consume_deeplink() -> Option<DeepLink> {
        Self::value().or_else(|| {
            let deeplink = Self::try_load_from_file()?;
            // Keep it in memory so a retried launch still sees the deep link
            Self::try_store_in_memory(deeplink.clone());
            Some(deeplink)
        })
    }

    /// Removes the persisted deep link file after a successful delivery.
    ///
    /// Keeps the file when it holds a different deep link (e.g. written by
    /// another launcher instance after this one consumed its own).
    pub fn clear_persisted_deeplink(consumed_url: &str) {
        if let Some(persisted) = Self::try_read_persisted() {
            if persisted.url == consumed_url {
                Self::try_clear_file();
            } else {
                info!("Keeping persisted deep link file: it holds a different deep link");
            }
        }
    }

    pub fn try_assign_value(&self, value: String) {
        match DeepLink::new(value) {
            Ok(deeplink) => Self::assign(deeplink),
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

    fn assign(deeplink: DeepLink) {
        // Persist so the deep link survives the self-update restart:
        // update.install() exits the process on Windows before the in-memory
        // state can be carried over
        Self::try_save_to_file(&deeplink);
        Self::try_store_in_memory(deeplink);
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
        match serde_json::to_string(&PersistedDeepLink::new(deeplink)) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    error!("Failed to persist deep link to {}: {}", path.display(), e);
                } else {
                    info!("Persisted deep link to {}", path.display());
                }
            }
            Err(e) => {
                error!("Failed to serialize deep link state: {}", e);
            }
        }
    }

    /// Reads the raw persisted state.
    ///
    /// A file that cannot be parsed is removed on the spot — leaving it
    /// would re-trigger the same error on every launch with no recovery
    /// path.
    fn try_read_persisted() -> Option<PersistedDeepLink> {
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

        match serde_json::from_str(&content) {
            Ok(persisted) => Some(persisted),
            Err(e) => {
                error!(
                    "Persisted deep link file {} is corrupt, removing it: {}",
                    path.display(),
                    e
                );
                Self::try_clear_file();
                None
            }
        }
    }

    fn try_load_from_file() -> Option<DeepLink> {
        let persisted = Self::try_read_persisted()?;

        if persisted.is_expired() {
            warn!("Ignoring expired persisted deep link, removing it");
            Self::try_clear_file();
            return None;
        }

        match DeepLink::new(persisted.url) {
            Ok(deeplink) => {
                info!("Loaded persisted deep link");
                Some(deeplink)
            }
            Err(DeepLinkCreateError::WrongPrefix { original_content }) => {
                error!(
                    "persisted deep link doesn't start with prefix protocol {}, removing it: {}",
                    PROTOCOL_PREFIX, original_content
                );
                Self::try_clear_file();
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
                Self::assign(deeplink);
                return;
            }
        }

        warn!(
            "none of values starts with prefix protocol {}: {:?}",
            PROTOCOL_PREFIX, value
        );
    }
}
