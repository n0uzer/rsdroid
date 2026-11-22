//! Typed VM configuration.
//!
//! M1 boots the pinned Android-x86 ISO live: the kernel and initrd are extracted
//! from the ISO and `VIRT_WIFI=0` is passed on the kernel command line. The
//! installed base image and qcow2 overlays arrive in M3/M5.

use std::path::{Path, PathBuf};

use crate::error::VmError;

/// Default CPU model that works under WHPX on the reference hardware.
///
/// `qemu64` alone lacks SSE4.2 and leaves Android 9 on a black screen; `max`
/// crashes QEMU under WHPX on the AMD host used for ADR 0001.
pub const DEFAULT_CPU_MODEL: &str = "qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt";
/// Default number of virtual CPUs.
pub const DEFAULT_CPUS: u8 = 2;
/// Default guest RAM in MiB.
pub const DEFAULT_MEMORY_MB: u32 = 3072;
/// Default host port forwarded to guest ADB port 5555.
pub const DEFAULT_ADB_PORT: u16 = 15555;
/// Default loopback port for QMP.
pub const DEFAULT_QMP_PORT: u16 = 14444;
/// Default loopback port for the guest serial console.
pub const DEFAULT_SERIAL_PORT: u16 = 14555;

/// How QEMU presents the guest display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// No QEMU window; video comes from scrcpy in M2.
    Headless,
    /// QEMU's own SDL window, for troubleshooting boot problems.
    Window,
}

/// Everything needed to launch one headless VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmConfig {
    /// Instance name; safe as a directory name.
    pub name: String,
    /// Path to `qemu-system-x86_64.exe`.
    pub qemu: PathBuf,
    /// Kernel extracted from the Android-x86 ISO.
    pub kernel: PathBuf,
    /// Initrd extracted from the Android-x86 ISO.
    pub initrd: PathBuf,
    /// Android-x86 ISO.
    pub iso: PathBuf,
    /// Number of virtual CPUs.
    pub cpus: u8,
    /// Guest RAM in MiB.
    pub memory_mb: u32,
    /// QEMU `-cpu` model string.
    pub cpu_model: String,
    /// Host loopback port forwarded to guest port 5555.
    pub adb_port: u16,
    /// Host loopback port for QMP.
    pub qmp_port: u16,
    /// Host loopback port for the guest serial console.
    pub serial_port: u16,
    /// Headless or QEMU window.
    pub display: DisplayMode,
    /// QEMU pid file.
    pub pid_file: PathBuf,
    /// QEMU stdout/stderr log.
    pub log_file: PathBuf,
}

impl VmConfig {
    /// Check the configuration before any process is started.
    pub fn validate(&self) -> Result<(), VmError> {
        validate_name(&self.name)?;
        if self.cpus == 0 || self.cpus > 64 {
            return Err(VmError::Config("cpus must be between 1 and 64".to_owned()));
        }
        if !(512..=65_536).contains(&self.memory_mb) {
            return Err(VmError::Config(
                "memory_mb must be between 512 and 65536".to_owned(),
            ));
        }
        if self.cpu_model.trim().is_empty() {
            return Err(VmError::Config("cpu_model must not be empty".to_owned()));
        }
        if !self
            .cpu_model
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ',' | '+' | '.' | '-' | '_'))
        {
            return Err(VmError::Config(
                "cpu_model contains characters QEMU would misparse".to_owned(),
            ));
        }
        let ports = [self.adb_port, self.qmp_port, self.serial_port];
        if ports.contains(&0) {
            return Err(VmError::Config("ports must not be zero".to_owned()));
        }
        if self.adb_port == self.qmp_port
            || self.adb_port == self.serial_port
            || self.qmp_port == self.serial_port
        {
            return Err(VmError::Config(
                "adb_port, qmp_port and serial_port must be different".to_owned(),
            ));
        }
        for (label, path) in [
            ("qemu", &self.qemu),
            ("kernel", &self.kernel),
            ("initrd", &self.initrd),
            ("iso", &self.iso),
        ] {
            if !path.is_file() {
                return Err(VmError::Config(format!(
                    "{label} does not exist or is not a file: {}",
                    path.display()
                )));
            }
        }
        if self.pid_file.as_os_str().is_empty() || self.log_file.as_os_str().is_empty() {
            return Err(VmError::Config(
                "pid_file and log_file must not be empty".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Validate an instance name before using it in a path.
pub fn validate_name(name: &str) -> Result<(), VmError> {
    if name.is_empty() || name.len() > 64 {
        return Err(VmError::Config(
            "instance name must be 1-64 characters".to_owned(),
        ));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(VmError::Config(
            "instance name may contain only letters, digits, '-' and '_'".to_owned(),
        ));
    }
    Ok(())
}

/// Directory that holds the ephemeral M1 run state for an instance.
pub fn run_dir(data_dir: &Path, name: &str) -> Result<PathBuf, VmError> {
    validate_name(name)?;
    Ok(data_dir.join("run").join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        (
            PathBuf::from(r"C:\qemu\qemu-system-x86_64.exe"),
            PathBuf::from(r"C:\images\kernel"),
            PathBuf::from(r"C:\images\initrd.img"),
            PathBuf::from(r"C:\images\android.iso"),
        )
    }

    fn config() -> VmConfig {
        let (qemu, kernel, initrd, iso) = paths();
        VmConfig {
            name: "test-1".to_owned(),
            qemu,
            kernel,
            initrd,
            iso,
            cpus: DEFAULT_CPUS,
            memory_mb: DEFAULT_MEMORY_MB,
            cpu_model: DEFAULT_CPU_MODEL.to_owned(),
            adb_port: DEFAULT_ADB_PORT,
            qmp_port: DEFAULT_QMP_PORT,
            serial_port: DEFAULT_SERIAL_PORT,
            display: DisplayMode::Headless,
            pid_file: PathBuf::from(r"C:\run\qemu.pid"),
            log_file: PathBuf::from(r"C:\run\qemu.log"),
        }
    }

    #[test]
    fn valid_names_are_accepted() {
        for name in ["a", "instance-1", "test_2"] {
            assert!(validate_name(name).is_ok());
        }
    }

    #[test]
    fn unsafe_names_are_rejected() {
        for name in ["", "..", "a/b", r"a\b", "with space", "café"] {
            assert!(validate_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn duplicate_ports_are_rejected() {
        let mut config = config();
        config.serial_port = config.qmp_port;
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("must be different"), "{error}");
    }
}
