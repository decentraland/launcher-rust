use std::sync::Mutex;

use log::{error, warn};

static PROTOCOL_STATE: Mutex<Option<String>> = Mutex::new(None);
const PROTOCOL_PREFIX: &str = "decentraland://";

pub struct Protocol {}

impl Protocol {
    pub fn value() -> Option<String> {
        let result = PROTOCOL_STATE.lock();
        match result {
            Ok(guard) => guard.clone(),
            Err(e) => {
                error!("cannot acquire mutex of PROTOCOL_STATE: {}", e);
                None
            }
        }
    }

    pub fn try_assign_value(value: String) {
        if !value.starts_with(PROTOCOL_PREFIX) {
            error!(
                "trying assing value that doesn't start with prefix protocol {}: {}",
                PROTOCOL_PREFIX, value
            );
            return;
        }

        let result = PROTOCOL_STATE.lock();
        match result {
            Ok(guard) => {
                let mut guard = guard;
                *guard = Some(value);
            }
            Err(e) => {
                error!("cannot acquire mutex of PROTOCOL_STATE: {}", e);
            }
        }
    }

    pub fn try_assign_value_from_vec(value: &Vec<String>) {
        for v in value {
            if v.starts_with(PROTOCOL_PREFIX) {
                Self::try_assign_value(v.to_owned());
                return;
            }
        }

        warn!(
            "none of values starts with prefix protocol {}: {:?}",
            PROTOCOL_PREFIX, value
        );
    }
}
