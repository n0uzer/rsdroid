//! Build the QEMU command line from a typed configuration.
//!
//! Every flag used here was checked against `qemu-system-x86_64 -help` from the
//! pinned QEMU 11.1.0 build and against the working line recorded in ADR 0001.

use std::ffi::OsString;

use crate::config::{DisplayMode, VmConfig};

/// Kernel command line for a live Android-x86 boot.
///
/// `VIRT_WIFI=0` keeps `eth0` instead of renaming it to `wifi_eth`, so Android's
/// Ethernet stack gets DHCP and adb can reach the guest.
pub const ANDROID_APPEND: &str = "root=/dev/ram0 quiet VIRT_WIFI=0 SRC= DATA=";

/// Build the argument vector passed to QEMU.
pub fn build_arguments(config: &VmConfig) -> Vec<OsString> {
    let mut args = Vec::new();
    macro_rules! arg {
        ($value:expr) => {
            args.push(OsString::from($value))
        };
    }

    arg!("-name");
    arg!(format!("rsdroid-{}", config.name));
    arg!("-machine");
    arg!("q35");
    arg!("-accel");
    arg!("whpx");
    arg!("-cpu");
    arg!(config.cpu_model.clone());
    arg!("-smp");
    arg!(config.cpus.to_string());
    arg!("-m");
    arg!(config.memory_mb.to_string());
    arg!("-kernel");
    args.push(config.kernel.clone().into_os_string());
    arg!("-initrd");
    args.push(config.initrd.clone().into_os_string());
    arg!("-append");
    arg!(ANDROID_APPEND);
    arg!("-cdrom");
    args.push(config.iso.clone().into_os_string());
    arg!("-netdev");
    arg!(format!(
        "user,id=n0,hostfwd=tcp:127.0.0.1:{}-:5555",
        config.adb_port
    ));
    arg!("-device");
    arg!("virtio-net-pci,netdev=n0");
    arg!("-vga");
    arg!("std");
    arg!("-display");
    arg!(match config.display {
        DisplayMode::Headless => "none",
        DisplayMode::Window => "sdl",
    });
    arg!("-qmp");
    arg!(format!(
        "tcp:127.0.0.1:{},server=on,wait=off",
        config.qmp_port
    ));
    arg!("-serial");
    arg!(format!(
        "tcp:127.0.0.1:{},server=on,wait=off",
        config.serial_port
    ));
    arg!("-pidfile");
    args.push(config.pid_file.clone().into_os_string());
    args
}

/// Render the command line for logs and `--dry-run`.
pub fn render(config: &VmConfig) -> String {
    let mut parts = Vec::with_capacity(1);
    parts.push(config.qemu.display().to_string());
    for arg in build_arguments(config) {
        let text = arg.to_string_lossy();
        if text.contains(' ') {
            parts.push(format!("\"{text}\""));
        } else {
            parts.push(text.into_owned());
        }
    }
    parts.join(" ")
}

/// The guest serial device address used in log messages.
pub fn adb_serial(config: &VmConfig) -> String {
    format!("127.0.0.1:{}", config.adb_port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DEFAULT_ADB_PORT, DEFAULT_CPUS, DEFAULT_CPU_MODEL, DEFAULT_MEMORY_MB, DEFAULT_QMP_PORT,
        DEFAULT_SERIAL_PORT,
    };
    use std::path::PathBuf;

    fn config(display: DisplayMode) -> VmConfig {
        VmConfig {
            name: "demo".to_owned(),
            qemu: PathBuf::from(r"C:\qemu\qemu-system-x86_64.exe"),
            kernel: PathBuf::from(r"C:\img\kernel"),
            initrd: PathBuf::from(r"C:\img\initrd.img"),
            iso: PathBuf::from(r"C:\img\android.iso"),
            cpus: DEFAULT_CPUS,
            memory_mb: DEFAULT_MEMORY_MB,
            cpu_model: DEFAULT_CPU_MODEL.to_owned(),
            adb_port: DEFAULT_ADB_PORT,
            qmp_port: DEFAULT_QMP_PORT,
            serial_port: DEFAULT_SERIAL_PORT,
            display,
            pid_file: PathBuf::from(r"C:\run\qemu.pid"),
            log_file: PathBuf::from(r"C:\run\qemu.log"),
        }
    }

    #[test]
    fn command_contains_the_verified_flags() {
        let args: Vec<String> = build_arguments(&config(DisplayMode::Headless))
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        for expected in [
            "-machine",
            "q35",
            "-accel",
            "whpx",
            "-cpu",
            DEFAULT_CPU_MODEL,
            "-smp",
            "2",
            "-m",
            "3072",
            "-append",
            ANDROID_APPEND,
            "user,id=n0,hostfwd=tcp:127.0.0.1:15555-:5555",
            "virtio-net-pci,netdev=n0",
            "-display",
            "none",
            "tcp:127.0.0.1:14444,server=on,wait=off",
            "tcp:127.0.0.1:14555,server=on,wait=off",
        ] {
            assert!(
                args.iter().any(|arg| arg == expected),
                "missing {expected:?} in {args:?}"
            );
        }
    }

    #[test]
    fn debug_window_uses_sdl() {
        let args = build_arguments(&config(DisplayMode::Window));
        assert!(args.iter().any(|arg| arg == "sdl"));
        assert!(!args.iter().any(|arg| arg == "none"));
    }

    #[test]
    fn render_quotes_the_kernel_command_line() {
        let rendered = render(&config(DisplayMode::Headless));
        assert!(rendered.contains("\"root=/dev/ram0 quiet VIRT_WIFI=0 SRC= DATA=\""));
        assert!(rendered.contains("demo"));
    }
}
