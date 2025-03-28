use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{CREATE_NEW_CONSOLE, DETACHED_PROCESS};

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use nix::unistd::setsid;

pub trait CommandExtDetached {
    fn detached(&mut self) -> &mut Self;
}

impl CommandExtDetached for Command {
    fn detached(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            self.creation_flags(CREATE_NEW_CONSOLE | DETACHED_PROCESS);
        }

        #[cfg(unix)]
        {
            unsafe {
                self.before_exec(|| {
                    unsafe { setsid() };
                    Ok(())
                });
            }
        }

        self
    }
}
