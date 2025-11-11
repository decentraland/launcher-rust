use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::{
    collections::HashMap,
    ffi::OsStr,
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

    #[cfg(target_os = "macos")]
    pub fn register_new_opened_instance_by_name(&self, process_name: &str) {
        let system = sysinfo::System::new_all();
        let exact_name = OsStr::new(process_name);
        let mut content: Storage = Self::file_content(self.path.as_path());
        let initial_count = content.processes.len();

        for candidate in system.processes_by_exact_name(exact_name) {
            let raw_pid = candidate.pid().as_u32();

            if let Entry::Vacant(e) = content.processes.entry(raw_pid) {
                let name = candidate
                    .name()
                    .to_str()
                    .unwrap_or("cannot parse os string");
                e.insert(name.to_owned());
                log::info!(
                    "Registered process run with id: {} and name {}",
                    raw_pid,
                    name
                );
            }
        }

        if initial_count == content.processes.len() {
            log::error!(
                "No new running instances were found. Likely instance is crashed or closed if it has been launched recently"
            );
        } else if let Err(e) = Self::write_content(&self.path, &content) {
            log::error!("Cannot register running instance: {:#?}", e);
        }
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
