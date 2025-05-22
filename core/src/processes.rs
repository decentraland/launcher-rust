use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{CREATE_NEW_CONSOLE, DETACHED_PROCESS};

#[cfg(unix)]
use nix::unistd::setsid;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

pub trait CommandExtDetached {
    fn detached(&mut self) -> &mut Self;
}

impl CommandExtDetached for Command {
    #[allow(deprecated)]
    fn detached(&mut self) -> &mut Self {
        #[cfg(unix)]
        {
            unsafe {
                self.before_exec(|| {
                    let _ = setsid();
                    Ok(())
                });
            }
        }

        self
    }
}
