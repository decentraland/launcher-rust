use std::result::Result;
use std::sync::Mutex;

use log::{error, warn};

static PROTOCOL_STATE: Mutex<Option<DeepLink>> = Mutex::new(None);
const PROTOCOL_PREFIX: &str = "decentraland://";

#[derive(Default, Clone)]
pub struct Protocol {}

pub enum DeepLinkCreateError {
    WrongPrefix { original_content: String },
}

#[derive(Clone)]
pub struct DeepLink(String);

impl DeepLink {
    fn new(value: String) -> Result<Self, DeepLinkCreateError> {
        if !value.starts_with(PROTOCOL_PREFIX) {
            Err(DeepLinkCreateError::WrongPrefix {
                original_content: value,
            })
        } else {
            Ok(DeepLink(value))
        }
    }
}

impl From<DeepLink> for String {
    fn from(deeplink: DeepLink) -> Self {
        deeplink.0
    }
}

impl Protocol {
    pub fn new() -> Self {
        Self {}
    }

    pub fn value(&self) -> Option<DeepLink> {
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
}
