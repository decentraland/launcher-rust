//! Blocks until a process the watchdog did not spawn exits, and says whether that exit was clean.
//!
//! macOS: kqueue `EVFILT_PROC` with `NOTE_EXITSTATUS` delivers the wait status of any process
//! of the same user. Windows: a process handle opened with `SYNCHRONIZE` keeps the exit code
//! readable after the process is gone.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitOutcome {
    Clean,
    /// Human readable exit description, stored verbatim in the crash attachment.
    Unexpected {
        code: String,
    },
}

pub use imp::wait_for_exit;

#[cfg(target_os = "macos")]
mod imp {
    use super::ExitOutcome;
    use dcl_launcher_core::anyhow::{Result, anyhow};
    use nix::errno::Errno;
    use nix::sys::event::{EventFilter, EventFlag, FilterFlag, KEvent, Kqueue};
    use nix::sys::wait::WaitStatus;
    use nix::unistd::Pid;

    pub fn wait_for_exit(pid: u32) -> Result<ExitOutcome> {
        let kqueue = Kqueue::new()?;
        let watch = KEvent::new(
            usize::try_from(pid)?,
            EventFilter::EVFILT_PROC,
            EventFlag::EV_ADD | EventFlag::EV_ONESHOT,
            FilterFlag::NOTE_EXIT | FilterFlag::NOTE_EXITSTATUS,
            0,
            0,
        );
        let mut events = [KEvent::new(
            0,
            EventFilter::EVFILT_PROC,
            EventFlag::empty(),
            FilterFlag::empty(),
            0,
            0,
        )];

        let received = match kqueue.kevent(&[watch], &mut events, None) {
            Ok(received) => received,
            Err(Errno::ESRCH) => {
                return Err(anyhow!("Process {pid} exited before the watchdog attached"));
            }
            Err(e) => return Err(e.into()),
        };
        if received == 0 {
            return Err(anyhow!("kevent returned without an event for pid {pid}"));
        }

        let event = events
            .first()
            .ok_or_else(|| anyhow!("kevent event list is empty"))?;
        if event.flags().contains(EventFlag::EV_ERROR) {
            let errno = Errno::from_raw(i32::try_from(event.data())?);
            return Err(anyhow!("kevent failed for pid {pid}: {errno}"));
        }

        let status = WaitStatus::from_raw(
            Pid::from_raw(i32::try_from(pid)?),
            i32::try_from(event.data())?,
        )?;
        Ok(classify(status))
    }

    pub fn classify(status: WaitStatus) -> ExitOutcome {
        match status {
            WaitStatus::Exited(_, 0) => ExitOutcome::Clean,
            WaitStatus::Exited(_, code) => ExitOutcome::Unexpected {
                code: format!("exit {code}"),
            },
            WaitStatus::Signaled(_, signal, core_dumped) => ExitOutcome::Unexpected {
                code: if core_dumped {
                    format!("signal {signal:?} (core dumped)")
                } else {
                    format!("signal {signal:?}")
                },
            },
            other => ExitOutcome::Unexpected {
                code: format!("{other:?}"),
            },
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use nix::sys::signal::Signal;

        #[test]
        fn zero_exit_is_clean_everything_else_is_not() {
            let pid = Pid::from_raw(1);
            assert_eq!(classify(WaitStatus::Exited(pid, 0)), ExitOutcome::Clean);
            assert_eq!(
                classify(WaitStatus::Exited(pid, 3)),
                ExitOutcome::Unexpected {
                    code: "exit 3".to_owned()
                }
            );
            assert_eq!(
                classify(WaitStatus::Signaled(pid, Signal::SIGSEGV, true)),
                ExitOutcome::Unexpected {
                    code: "signal SIGSEGV (core dumped)".to_owned()
                }
            );
        }

        #[test]
        fn waiting_on_a_finished_child_reports_its_status() {
            let mut child = std::process::Command::new("sh")
                .args(["-c", "exit 7"])
                .spawn()
                .unwrap_or_else(|e| panic!("{e}"));
            let outcome = wait_for_exit(child.id()).unwrap_or_else(|e| panic!("{e}"));
            // kqueue reports the exit; the zombie is still ours to reap.
            let _ = child.wait();
            assert_eq!(
                outcome,
                ExitOutcome::Unexpected {
                    code: "exit 7".to_owned()
                }
            );
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::ExitOutcome;
    use dcl_launcher_core::anyhow::{Result, anyhow};
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, SYNCHRONIZE,
        WaitForSingleObject,
    };

    /// Exit codes at or above this are NTSTATUS failures (e.g. `0xC0000005` access violation).
    const NTSTATUS_FAILURE_BASE: u32 = 0x8000_0000;

    // The unsafe blocks are plain Win32 calls with a handle we own and close on every path.
    #[allow(unsafe_code)]
    pub fn wait_for_exit(pid: u32) -> Result<ExitOutcome> {
        let handle =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return Err(anyhow!(
                "OpenProcess({pid}) failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let waited = unsafe { WaitForSingleObject(handle, INFINITE) };
        if waited != WAIT_OBJECT_0 {
            let error = std::io::Error::last_os_error();
            unsafe { CloseHandle(handle) };
            return Err(anyhow!(
                "WaitForSingleObject({pid}) returned {waited}: {error}"
            ));
        }

        let mut code: u32 = 0;
        let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
        let error = std::io::Error::last_os_error();
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return Err(anyhow!("GetExitCodeProcess({pid}) failed: {error}"));
        }

        Ok(classify(code))
    }

    pub fn classify(code: u32) -> ExitOutcome {
        if code == 0 {
            ExitOutcome::Clean
        } else if code >= NTSTATUS_FAILURE_BASE {
            ExitOutcome::Unexpected {
                code: format!("0x{code:08X}"),
            }
        } else {
            ExitOutcome::Unexpected {
                code: format!("exit {code}"),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn zero_is_clean_ntstatus_is_hex_others_decimal() {
            assert_eq!(classify(0), ExitOutcome::Clean);
            assert_eq!(
                classify(1),
                ExitOutcome::Unexpected {
                    code: "exit 1".to_owned()
                }
            );
            assert_eq!(
                classify(0xC000_0005),
                ExitOutcome::Unexpected {
                    code: "0xC0000005".to_owned()
                }
            );
        }

        #[test]
        fn waiting_on_a_finished_child_reports_its_status() {
            let mut child = std::process::Command::new("cmd")
                .args(["/C", "exit 7"])
                .spawn()
                .unwrap_or_else(|e| panic!("{e}"));
            let outcome = wait_for_exit(child.id()).unwrap_or_else(|e| panic!("{e}"));
            // WaitForSingleObject reports the exit; the zombie is still ours to reap.
            let _ = child.wait();
            assert_eq!(
                outcome,
                ExitOutcome::Unexpected {
                    code: "exit 7".to_owned()
                }
            );
        }
    }
}
