use std::process::Command;

#[cfg(unix)]
use nix::unistd::setsid;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

pub trait CommandExtDetached {
    #[allow(dead_code)]
    fn detached(&mut self) -> &mut Self;
}

impl CommandExtDetached for Command {
    #[allow(deprecated)]
    fn detached(&mut self) -> &mut Self {
        #[cfg(unix)]
        {
            #![allow(unsafe_code)]
            unsafe {
                self.before_exec(|| {
                    let _ = setsid();
                    Ok(())
                })
            }
        }

        #[cfg(windows)]
        self
    }
}
