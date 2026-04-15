#[cfg(windows)]
use std::process::Command;

#[cfg(windows)]
pub trait CommandExtDetached {
    fn detached(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl CommandExtDetached for Command {
    fn detached(&mut self) -> &mut Self {
        self
    }
}
