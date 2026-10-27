use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
};
use sysinfo::Pid;

use crate::installs;

pub struct RunningInstances {
    path: PathBuf,
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct Storage {
    pub processes: HashMap<u32, String>,
}

impl RunningInstances {
    #[cfg(target_os = "windows")]
    pub fn register_instance(&self, pid_raw: u32) {
        let system = sysinfo::System::new_all();
        let pid = Pid::from_u32(pid_raw);

        let process = system.process(pid);
        let name = match process {
            Some(p) => p.name().to_str().unwrap_or("cannot parse os string"),
            None => "no name found",
        };
        log::info!("Process run with id: {} and name {}", pid_raw, name);

        if let Err(e) = self.write_to_json_file(pid_raw, name) {
            log::error!("Cannot register running instance: {:#?}", e);
        }
    }

    /// Registers every not-yet-known process under `app_path` and returns the newly registered
    /// `(pid, exe_path)` pairs; empty when nothing new appeared this poll.
    #[cfg(target_os = "macos")]
    pub fn register_new_opened_instances_by_fuzzy_path(
        &self,
        app_path: &Path,
    ) -> Vec<(u32, PathBuf)> {
        use std::collections::hash_map::Entry;

        let mut content: Storage = Self::file_content(self.path.as_path());
        let mut registered = Vec::new();

        for (raw_pid, name, exe_path) in Self::processes_under_path(app_path) {
            if let Entry::Vacant(e) = content.processes.entry(raw_pid) {
                log::info!(
                    "Accepted and registered process pid {} name {} exe {}",
                    raw_pid,
                    name,
                    exe_path.display()
                );
                e.insert(name);
                registered.push((raw_pid, exe_path));
            }
        }

        if registered.is_empty() {
            log::info!(
                "No new Explorer instances found this poll under {}",
                app_path.display()
            );
        } else if let Err(e) = Self::write_content(&self.path, &content) {
            log::error!("Cannot persist running instance(s): {:#?}", e);
        }
        registered
    }

    /// Scans the OS process list for every process whose executable lives under
    /// `path`, yielding `(raw_pid, name, exe_path)` for each match.
    #[cfg(target_os = "macos")]
    fn processes_under_path(path: &Path) -> Vec<(u32, String, PathBuf)> {
        let system = sysinfo::System::new_all();
        let mut found = Vec::new();

        for (pid, process) in system.processes() {
            let Some(exe_path) = process.exe() else {
                continue;
            };
            if exe_path.starts_with(path) {
                let name = process
                    .name()
                    .to_str()
                    .unwrap_or("cannot parse os string")
                    .to_owned();
                found.push((pid.as_u32(), name, exe_path.to_path_buf()));
            }
        }
        found
    }

    /// Scans the OS process list for any process whose executable or working
    /// directory is under the `latest/` Explorer directory. Returns
    /// "name (pid, reason)" strings for each match.
    /// Authoritative — does not depend on the `running-instances.json` tracker,
    /// which can hold stale PIDs across sessions.
    pub fn explorer_processes_by_path(&self) -> Vec<String> {
        let latest_path = installs::explorer_latest_version_path();
        let system = sysinfo::System::new_all();
        system
            .processes()
            .iter()
            .filter_map(|(pid, process)| {
                let reason = why_process_holds_dir(process, &latest_path)?;
                let name = process.name().to_str().unwrap_or("cannot parse os string");
                Some(format!("{name} (pid {pid}, {reason})"))
            })
            .collect()
    }

    pub fn any_is_running(&self) -> Result<bool> {
        let system = sysinfo::System::new_all();
        let mut content = Self::file_content(self.path.as_path());
        let mut dead_process_pids: Vec<u32> = Vec::new();

        let mut any_running = false;

        for (id, name) in &content.processes {
            let id = id.to_owned();
            let pid = Pid::from_u32(id);

            if let Some(process) = system.process(pid) {
                match process.name().to_str() {
                    Some(valid) => {
                        log::info!("Instance pid name is valid {}: {}", pid, valid);
                        if valid == name.as_str() {
                            any_running = true;
                        } else {
                            dead_process_pids.push(id);
                        }
                    }
                    None => {
                        log::info!("Instance pid name is invalid {}", pid);
                        dead_process_pids.push(id);
                    }
                }
            } else {
                log::info!("Instance pid is dead {}", pid);
                dead_process_pids.push(id);
            }
        }

        if !dead_process_pids.is_empty() {
            for pid in &dead_process_pids {
                content.processes.remove(pid);
            }
            Self::write_content(self.path.as_path(), &content)?;
        }

        log::info!("Any running instances {}", any_running);
        Ok(any_running)
    }

    fn file_content(path: &Path) -> Storage {
        match File::open(path) {
            Ok(file) => {
                let storage = serde_json::from_reader(file).unwrap_or_else(|e| {
                    log::error!(
                        "Cannot read storage from file, fallback to default storage: {}",
                        e
                    );
                    Storage::default()
                });
                log::info!("Storage from file: {:#?}", storage);
                storage
            }
            Err(_) => Storage::default(),
        }
    }

    fn write_content(path: &Path, storage: &Storage) -> Result<()> {
        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, storage)?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn write_to_json_file(&self, pid: u32, name: &str) -> Result<()> {
        let path = self.path.as_path();
        let mut content: Storage = Self::file_content(path);
        content.processes.insert(pid, name.to_owned());
        Self::write_content(path, &content)?;
        Ok(())
    }
}

impl Default for RunningInstances {
    fn default() -> Self {
        //TODO clean dead processes in json file on start
        Self {
            path: installs::running_instances_path(),
        }
    }
}

/// Why `process` keeps `dir` from being renamed or deleted, if it does.
fn why_process_holds_dir(process: &sysinfo::Process, dir: &Path) -> Option<&'static str> {
    if process
        .exe()
        .is_some_and(|exe| installs::is_inside_dir(exe, dir))
    {
        return Some("runs from it");
    }
    if process
        .cwd()
        .is_some_and(|cwd| installs::is_inside_dir(cwd, dir))
    {
        return Some("working directory");
    }
    None
}
