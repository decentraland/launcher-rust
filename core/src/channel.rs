use anyhow::Result;

use dcl_launcher_shared::types::Status;

pub trait EventChannel {
    fn send(&self, status: Status) -> Result<()>;
}
