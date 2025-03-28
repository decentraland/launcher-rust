use anyhow::Result;

use crate::types::Status;

pub trait EventChannel {
    fn send(&self, status: Status) -> Result<()>;
}
