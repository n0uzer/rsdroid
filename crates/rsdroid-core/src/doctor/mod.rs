//! Environment diagnostics for `rsdroid doctor`.
//!
//! Checks are independent and never return `Err`: a broken probe becomes a
//! finding in the report, so one missing tool cannot hide the others.

mod bundle;
mod report;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;
use std::time::Duration;

pub use bundle::{write_bundle, BundleOptions};
pub use report::{
    Check, CheckId, CheckStatus, PlatformInfo, Report, Summary, REPORT_SCHEMA_VERSION,
};

/// Inputs that change where doctor looks for external tools and files.
#[derive(Debug, Clone)]
pub struct DoctorOptions {
    /// Explicit path to `qemu-system-x86_64.exe`.
    pub qemu: Option<PathBuf>,
    /// Explicit path to `adb.exe`.
    pub adb: Option<PathBuf>,
    /// Override for `%LOCALAPPDATA%\rsdroid`.
    pub data_dir: Option<PathBuf>,
    /// Per-command timeout for version probes.
    pub timeout: Duration,
}

impl Default for DoctorOptions {
    fn default() -> Self {
        Self {
            qemu: None,
            adb: None,
            data_dir: None,
            timeout: Duration::from_secs(10),
        }
    }
}

/// Run all checks and return a report. This never fails.
pub fn run(options: &DoctorOptions) -> Report {
    #[cfg(windows)]
    {
        windows::run(options)
    }
    #[cfg(not(windows))]
    {
        unsupported_platform()
    }
}

#[cfg(not(windows))]
fn unsupported_platform() -> Report {
    let platform = PlatformInfo {
        os_name: std::env::consts::OS.to_owned(),
        os_version: std::env::consts::OS.to_owned(),
        os_build: 0,
        architecture: std::env::consts::ARCH.to_owned(),
        cpu: "unknown".to_owned(),
        cpu_cores: 0,
        memory_mb: 0,
        hypervisor_present: false,
    };
    let checks = vec![Check::skipped(
        CheckId::WindowsVersion,
        "Windows version",
        "rsdroid targets Windows 10 22H2 and Windows 11; this host is not Windows.",
    )];
    Report::new(platform, checks)
}
