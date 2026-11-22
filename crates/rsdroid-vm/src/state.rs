//! Persisted run state shared by `rsdroid run` and `rsdroid stop`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::VmConfig;
use crate::error::VmError;

/// Schema version of `state.json`.
pub const STATE_SCHEMA_VERSION: u32 = 1;

/// The small state file written by `run` before QEMU is controlled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmState {
    /// Schema version, so M3 can migrate or reject old files.
    pub schema_version: u32,
    /// Instance name.
    pub name: String,
    /// PID of the QEMU process.
    pub qemu_pid: u32,
    /// QMP loopback port.
    pub qmp_port: u16,
    /// ADB loopback port.
    pub adb_port: u16,
    /// Serial console loopback port.
    pub serial_port: u16,
    /// QEMU log file.
    pub log_file: PathBuf,
}

impl VmState {
    /// Build state from a validated configuration and the spawned QEMU pid.
    pub fn new(config: &VmConfig, qemu_pid: u32) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            name: config.name.clone(),
            qemu_pid,
            qmp_port: config.qmp_port,
            adb_port: config.adb_port,
            serial_port: config.serial_port,
            log_file: config.log_file.clone(),
        }
    }

    /// `adb` serial for this VM.
    pub fn adb_serial(&self) -> String {
        format!("127.0.0.1:{}", self.adb_port)
    }
}

/// Path of the state file inside a run directory.
pub fn state_path(run_dir: &Path) -> PathBuf {
    run_dir.join("state.json")
}

/// Write `state.json`, creating the run directory.
pub fn write_state(run_dir: &Path, state: &VmState) -> Result<PathBuf, VmError> {
    fs::create_dir_all(run_dir).map_err(|error| VmError::io(run_dir, error))?;
    let path = state_path(run_dir);
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| VmError::State(format!("cannot serialize state: {error}")))?;
    fs::write(&temporary, bytes).map_err(|error| VmError::io(&temporary, error))?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| VmError::io(&path, error))?;
    }
    fs::rename(&temporary, &path).map_err(|error| VmError::io(&path, error))?;
    Ok(path)
}

/// Read `state.json` for an instance.
pub fn read_state(run_dir: &Path) -> Result<VmState, VmError> {
    let path = state_path(run_dir);
    let bytes = fs::read(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            VmError::State(format!(
                "instance '{}' is not running (no {})",
                run_dir
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default(),
                path.display()
            ))
        } else {
            VmError::io(&path, error)
        }
    })?;
    let state: VmState = serde_json::from_slice(&bytes)
        .map_err(|error| VmError::State(format!("cannot parse {}: {error}", path.display())))?;
    if state.schema_version != STATE_SCHEMA_VERSION {
        return Err(VmError::State(format!(
            "unsupported state schema {} in {}",
            state.schema_version,
            path.display()
        )));
    }
    Ok(state)
}

/// Remove `state.json` after a clean run.
pub fn remove_state(run_dir: &Path) -> Result<(), VmError> {
    let path = state_path(run_dir);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(VmError::io(&path, error)),
    }
}

/// Check whether a process id is still running.
#[cfg(windows)]
pub fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// `STILL_ACTIVE` from `winbase.h`; windows-sys does not export it.
    const STILL_ACTIVE: u32 = 259;

    // SAFETY: the pid comes from our own state file. The handle is closed before
    // returning and the out-parameter points to a valid local.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok != 0 && code == STILL_ACTIVE
    }
}

/// Check whether a process id is still running (non-Windows fallback).
#[cfg(not(windows))]
pub fn process_alive(_pid: u32) -> bool {
    true
}
