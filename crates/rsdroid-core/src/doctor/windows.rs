//! Windows probes used by `rsdroid doctor`.
//!
//! Every probe is best-effort: a missing API or registry value becomes a
//! finding, never a panic.
#![allow(unsafe_code)] // All Win32 calls are confined to this module and commented.

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::SystemInformation::{
    GetNativeSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
};
use windows_sys::Win32::System::Threading::{IsProcessorFeaturePresent, PROCESSOR_FEATURE_ID};
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

use super::{Check, CheckId, DoctorOptions, PlatformInfo, Report};

/// `PF_VIRT_FIRMWARE_ENABLED` from `winnt.h`; windows-sys does not expose it.
const PF_VIRT_FIRMWARE_ENABLED: PROCESSOR_FEATURE_ID = 21;
const PINNED_QEMU_VERSION: &str = "11.1.0";
const MIN_SUPPORTED_BUILD: u32 = 19_044;
const FIRST_WINDOWS11_BUILD: u32 = 22_000;

/// QEMU capability code 0 is `WHvCapabilityCodeHypervisorPresent`.
type WhvGetCapabilityFn = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;

#[repr(align(16))]
struct AlignedBuffer([u8; 4096]);

pub(super) fn run(options: &DoctorOptions) -> Report {
    let probe = probe_whpx();
    let firmware_virtualization = firmware_virtualization_enabled();
    let version = windows_version();
    let platform = PlatformInfo {
        os_name: version.product.clone(),
        os_version: version.display.clone(),
        os_build: version.build,
        architecture: std::env::consts::ARCH.to_owned(),
        cpu: cpu_name(),
        cpu_cores: cpu_cores(),
        memory_mb: memory_mb(),
        hypervisor_present: probe.present,
    };

    let checks = vec![
        check_windows_version(&version),
        check_cpu_virtualization(firmware_virtualization, probe.present),
        check_hypervisor_platform(&probe),
        check_qemu(options),
        check_adb(options),
        check_conflicts(),
        check_data_dir(options),
    ];
    Report::new(platform, checks)
}

#[derive(Debug, Clone)]
struct WindowsVersion {
    product: String,
    display: String,
    build: u32,
    ubr: u32,
    is_ltsc: bool,
}

fn windows_version() -> WindowsVersion {
    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let product = registry_string(KEY, "ProductName").unwrap_or_else(|| "Windows".to_owned());
    let build = registry_string(KEY, "CurrentBuildNumber")
        .and_then(|value| value.parse::<u32>().ok())
        .or_else(|| {
            registry_string(KEY, "CurrentBuild").and_then(|value| value.parse::<u32>().ok())
        })
        .unwrap_or(0);
    let ubr = registry_u32(KEY, "UBR").unwrap_or(0);
    let display_version = registry_string(KEY, "DisplayVersion").unwrap_or_default();
    let edition = registry_string(KEY, "EditionID").unwrap_or_default();
    let is_ltsc = edition.to_ascii_lowercase().contains("ltsc")
        || product.to_ascii_lowercase().contains("ltsc");

    let mut display = if display_version.is_empty() {
        format!("{build}.{ubr}")
    } else {
        format!("{display_version} ({build}.{ubr})")
    };
    if build == 0 {
        display = "unknown build".to_owned();
    }

    let product = if build >= FIRST_WINDOWS11_BUILD && product.contains("Windows 10") {
        product.replacen("Windows 10", "Windows 11", 1)
    } else {
        product
    };

    WindowsVersion {
        product,
        display,
        build,
        ubr,
        is_ltsc,
    }
}

fn check_windows_version(version: &WindowsVersion) -> Check {
    let title = "Windows version";
    if version.build == 0 {
        return Check::warning(
            CheckId::WindowsVersion,
            title,
            "The Windows build could not be read from the registry.",
        )
        .with_remediation(
            "rsdroid targets Windows 10 22H2 (build 19045) and Windows 11. Verify the Windows installation and retry.",
        );
    }
    if version.build < MIN_SUPPORTED_BUILD {
        return Check::error(
            CheckId::WindowsVersion,
            title,
            format!(
                "{} build {}.{} is older than Windows 10 21H2 (build 19044).",
                version.product, version.build, version.ubr
            ),
        )
        .with_remediation("Update Windows to version 22H2 or Windows 11 before running rsdroid.");
    }
    if version.build == MIN_SUPPORTED_BUILD && !version.is_ltsc {
        return Check::warning(
            CheckId::WindowsVersion,
            title,
            format!(
                "{} build {}.{} is Windows 10 21H2, below the documented 22H2 target.",
                version.product, version.build, version.ubr
            ),
        )
        .with_remediation(
            "Update to Windows 10 22H2 (build 19045) or Windows 11. LTSC 2021 is accepted as-is.",
        );
    }
    if version.build < FIRST_WINDOWS11_BUILD {
        let detail = if version.is_ltsc {
            format!(
                "{} build {}.{} is Windows 10 LTSC 2021; supported for development, validate on 22H2 before release.",
                version.product, version.build, version.ubr
            )
        } else {
            format!(
                "{} build {}.{} is Windows 10 22H2.",
                version.product, version.build, version.ubr
            )
        };
        return Check::ok(CheckId::WindowsVersion, title, detail);
    }
    Check::ok(
        CheckId::WindowsVersion,
        title,
        format!(
            "{} build {}.{} is Windows 11.",
            version.product, version.build, version.ubr
        ),
    )
}

fn firmware_virtualization_enabled() -> bool {
    // SAFETY: IsProcessorFeaturePresent takes a feature id and has no pointer
    // arguments. A false result is not conclusive while a hypervisor is active.
    unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) != 0 }
}

fn check_cpu_virtualization(firmware_enabled: bool, hypervisor_present: bool) -> Check {
    if firmware_enabled {
        return Check::ok(
            CheckId::CpuVirtualization,
            "CPU virtualization",
            "Windows reports VT-x/AMD-V as enabled by firmware.",
        );
    }
    if hypervisor_present {
        // Hyper-V, VBS and WHPX hide the firmware bit while they run. A present
        // hypervisor cannot work without VT-x/AMD-V, so this is a pass.
        return Check::ok(
            CheckId::CpuVirtualization,
            "CPU virtualization",
            "A hypervisor is active, so VT-x/AMD-V is enabled. Windows can report the firmware bit as disabled while Hyper-V or WHPX is running.",
        );
    }
    Check::error(
        CheckId::CpuVirtualization,
        "CPU virtualization",
        "Windows reports that firmware has not enabled VT-x/AMD-V, and no hypervisor is active.",
    )
    .with_remediation(
        "Enable Intel VT-x, Intel Virtualization Technology or AMD SVM in UEFI/BIOS, save, and reboot.",
    )
}

#[derive(Debug, Clone)]
struct WhpxProbe {
    present: bool,
    detail: String,
}

fn probe_whpx() -> WhpxProbe {
    // SAFETY: the DLL name and symbol name are NUL-terminated, and the function
    // signature matches WHvGetCapability from WinHvPlatform.h.
    unsafe {
        let library = match libloading::Library::new("WinHvPlatform.dll") {
            Ok(library) => library,
            Err(error) => {
                return WhpxProbe {
                    present: false,
                    detail: format!("WinHvPlatform.dll could not be loaded: {error}"),
                };
            }
        };
        let function: libloading::Symbol<'_, WhvGetCapabilityFn> = match library
            .get(b"WHvGetCapability\0")
        {
            Ok(function) => function,
            Err(error) => {
                return WhpxProbe {
                    present: false,
                    detail: format!("WHvGetCapability is missing from WinHvPlatform.dll: {error}"),
                };
            }
        };

        let mut buffer = AlignedBuffer([0u8; 4096]);
        let mut written = 0u32;
        let result = function(
            0,
            buffer.0.as_mut_ptr().cast(),
            buffer.0.len() as u32,
            &mut written,
        );
        if result < 0 {
            return WhpxProbe {
                present: false,
                detail: format!(
                    "WHvGetCapability failed with HRESULT 0x{:08X}.",
                    result as u32
                ),
            };
        }
        if written < 4 {
            return WhpxProbe {
                present: false,
                detail: format!("WHvGetCapability returned only {written} bytes."),
            };
        }
        let mut value = [0u8; 4];
        value.copy_from_slice(&buffer.0[..4]);
        let present = u32::from_ne_bytes(value) != 0;
        WhpxProbe {
            present,
            detail: if present {
                "WinHvPlatform.dll reports a present hypervisor.".to_owned()
            } else {
                "WinHvPlatform.dll is installed but reports no present hypervisor.".to_owned()
            },
        }
    }
}

fn check_hypervisor_platform(probe: &WhpxProbe) -> Check {
    if probe.present {
        Check::ok(
            CheckId::HypervisorPlatform,
            "Windows Hypervisor Platform (WHPX)",
            probe.detail.clone(),
        )
    } else {
        Check::error(
            CheckId::HypervisorPlatform,
            "Windows Hypervisor Platform (WHPX)",
            probe.detail.clone(),
        )
        .with_remediation(
            "Enable \"Windows Hypervisor Platform\" in optionalfeatures.exe and reboot.\n\
             As administrator: DISM /Online /Enable-Feature /FeatureName:HypervisorPlatform /All /NoRestart\n\
             Hyper-V and WSL2 may stay enabled; WHPX coexists with them.",
        )
    }
}

fn check_qemu(options: &DoctorOptions) -> Check {
    let title = "QEMU with WHPX";
    let Some(path) = find_qemu(options) else {
        return Check::error(
            CheckId::Qemu,
            title,
            "qemu-system-x86_64.exe was not found on PATH, in %LOCALAPPDATA%\\rsdroid\\tools\\qemu or in C:\\Program Files\\qemu.",
        )
        .with_remediation(
            "Download the pinned QEMU 11.1.0 installer with:\n\
             pwsh tools/fetch-deps.ps1 -Component qemu\n\
             Then install it to C:\\Program Files\\qemu, or pass --qemu <path>.",
        );
    };

    let version = match run_capture(&path, &["--version"], options.timeout) {
        Ok(output) => output.lines().next().unwrap_or("").trim().to_owned(),
        Err(error) => {
            return Check::error(
                CheckId::Qemu,
                title,
                format!(
                    "Found {} but could not read its version: {error}",
                    path.display()
                ),
            )
            .with_remediation("Reinstall the pinned QEMU build from tools/deps.json.");
        }
    };
    let accelerators = match run_capture(&path, &["-accel", "help"], options.timeout) {
        Ok(output) => output,
        Err(error) => {
            return Check::error(
                CheckId::Qemu,
                title,
                format!(
                    "Found {} but could not list accelerators: {error}",
                    path.display()
                ),
            );
        }
    };
    let whpx = accelerators.to_ascii_lowercase().contains("whpx");
    let qemu_img_missing = !path.with_file_name("qemu-img.exe").is_file();
    let version_pinned = version.contains(PINNED_QEMU_VERSION);

    if !whpx {
        return Check::error(
            CheckId::Qemu,
            title,
            format!(
                "{} does not list the whpx accelerator. Available: {}",
                path.display(),
                accelerators.trim()
            ),
        )
        .with_remediation(
            "Use the pinned QEMU 11.1.0 Windows build from tools/deps.json; older or custom builds may omit WHPX.",
        );
    }

    let detail = format!(
        "{version} at {}. WHPX accelerator available.{}",
        path.display(),
        if qemu_img_missing {
            " qemu-img.exe is missing next to the executable."
        } else {
            ""
        }
    );
    if !version_pinned || qemu_img_missing {
        Check::warning(CheckId::Qemu, title, detail).with_remediation(
            "Install the pinned QEMU 11.1.0 build from tools/deps.json so doctor, M1 and the release pipeline agree.",
        )
    } else {
        Check::ok(CheckId::Qemu, title, detail)
    }
}

fn check_adb(options: &DoctorOptions) -> Check {
    let title = "Android Debug Bridge (adb)";
    let Some(path) = find_adb(options) else {
        return Check::warning(
            CheckId::Adb,
            title,
            "adb.exe was not found on PATH or in the usual Android SDK locations.",
        )
        .with_remediation(
            "Install Android platform-tools and add its directory to PATH. M1 needs adb to wait for boot completion.",
        );
    };
    match run_capture(&path, &["version"], options.timeout) {
        Ok(output) => {
            let first = output.lines().next().unwrap_or("").trim();
            Check::ok(
                CheckId::Adb,
                title,
                format!("{first} at {}", path.display()),
            )
        }
        Err(error) => Check::warning(
            CheckId::Adb,
            title,
            format!("Found {} but could not run it: {error}", path.display()),
        )
        .with_remediation("Reinstall Android platform-tools."),
    }
}

fn check_conflicts() -> Check {
    let title = "Conflicting hypervisor software";
    let mut findings = BTreeSet::new();
    let programs = installed_programs();

    for (needle, label) in [
        ("virtualbox", "VirtualBox"),
        ("vmware workstation", "VMware Workstation"),
        ("vmware player", "VMware Player"),
        ("vmware workstations", "VMware Workstation"),
        ("bluestacks", "BlueStacks"),
        ("ldplayer", "LDPlayer"),
        ("noxplayer", "NoxPlayer"),
        ("memu", "MEmu"),
        ("genymotion", "Genymotion"),
    ] {
        if programs
            .iter()
            .any(|name| name.to_ascii_lowercase().contains(needle))
        {
            findings.insert(label.to_owned());
        }
    }

    for (service, label) in [
        ("VBoxSup", "VirtualBox driver"),
        ("VBoxDrv", "VirtualBox driver"),
        ("vmx86", "VMware driver"),
        ("IntelHaxm", "Intel HAXM"),
        ("gvm", "Android Emulator Hypervisor Driver"),
    ] {
        if service_exists(service) {
            findings.insert(label.to_owned());
        }
    }

    for process in running_processes() {
        let lower = process.to_ascii_lowercase();
        for (needle, label) in [
            ("vboxsvc.exe", "VirtualBox"),
            ("virtualboxvm.exe", "VirtualBox"),
            ("vmware-vmx.exe", "VMware"),
            ("vmware.exe", "VMware"),
            ("hd-player.exe", "BlueStacks"),
            ("dnplayer.exe", "LDPlayer"),
            ("nox.exe", "NoxPlayer"),
            ("memu.exe", "MEmu"),
            ("genymotion.exe", "Genymotion"),
        ] {
            if lower == needle {
                findings.insert(format!("{label} (running)"));
            }
        }
    }

    if findings.is_empty() {
        Check::ok(
            CheckId::ConflictingSoftware,
            title,
            "No other hypervisor or Android-emulator software was detected.",
        )
    } else {
        Check::warning(
            CheckId::ConflictingSoftware,
            title,
            format!(
                "Detected: {}. These programs can compete for VT-x/AMD-V or hide it from WHPX.",
                findings.into_iter().collect::<Vec<_>>().join(", ")
            ),
        )
        .with_remediation(
            "Close them before starting rsdroid. VirtualBox 7+ can use WHPX; older VirtualBox, BlueStacks and LDPlayer versions conflict with Hyper-V/WHPX.",
        )
    }
}

fn check_data_dir(options: &DoctorOptions) -> Check {
    let title = "rsdroid data directory";
    let directory = match &options.data_dir {
        Some(path) => path.clone(),
        None => match crate::paths::data_dir() {
            Ok(path) => path,
            Err(error) => {
                return Check::error(CheckId::DataDirectory, title, error.to_string())
                    .with_remediation(
                        "Set LOCALAPPDATA or pass --data-dir to a writable local directory.",
                    );
            }
        },
    };

    let probe = directory.join(format!(".doctor-{}.tmp", std::process::id()));
    let write_result = fs::create_dir_all(&directory)
        .and_then(|_| fs::write(&probe, b"rsdroid"))
        .and_then(|_| fs::read(&probe))
        .and_then(|contents| {
            let _ = fs::remove_file(&probe);
            if contents == b"rsdroid" {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "read-back mismatch",
                ))
            }
        });

    match write_result {
        Ok(()) => Check::ok(
            CheckId::DataDirectory,
            title,
            format!("{} is writable.", directory.display()),
        ),
        Err(error) => Check::error(
            CheckId::DataDirectory,
            title,
            format!("{} is not writable: {error}", directory.display()),
        )
        .with_remediation(
            "Pass --data-dir with a writable local path. Do not use a network share or a protected system directory.",
        ),
    }
}

fn find_qemu(options: &DoctorOptions) -> Option<PathBuf> {
    crate::tools::find_qemu(options.qemu.as_deref())
}

fn find_adb(options: &DoctorOptions) -> Option<PathBuf> {
    crate::tools::find_adb(options.adb.as_deref())
}

fn run_capture(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> std::result::Result<String, String> {
    let mut stdout =
        tempfile::tempfile().map_err(|error| format!("cannot create a temporary file: {error}"))?;
    let mut stderr =
        tempfile::tempfile().map_err(|error| format!("cannot create a temporary file: {error}"))?;
    let stdout_clone = stdout
        .try_clone()
        .map_err(|error| format!("cannot clone a temporary file handle: {error}"))?;
    let stderr_clone = stderr
        .try_clone()
        .map_err(|error| format!("cannot clone a temporary file handle: {error}"))?;

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_clone))
        .stderr(Stdio::from(stderr_clone))
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", program.display()))?;

    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{} timed out after {}s",
                program.display(),
                timeout.as_secs()
            ));
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("cannot wait for {}: {error}", program.display()));
        }
    };

    let output = read_temp(&mut stdout);
    let errors = read_temp(&mut stderr);
    let combined = if errors.trim().is_empty() {
        output
    } else if output.is_empty() {
        errors
    } else {
        format!("{output}\n{errors}")
    };

    if status.success() {
        Ok(combined)
    } else {
        Err(format!(
            "{} exited with {}: {}",
            program.display(),
            status.code().unwrap_or(-1),
            combined.trim()
        ))
    }
}

fn read_temp(file: &mut File) -> String {
    if file.seek(SeekFrom::Start(0)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn registry_string(path: &str, name: &str) -> Option<String> {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(path, KEY_READ)
        .ok()?
        .get_value::<String, _>(name)
        .ok()
}

fn registry_u32(path: &str, name: &str) -> Option<u32> {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(path, KEY_READ)
        .ok()?
        .get_value::<u32, _>(name)
        .ok()
}

fn cpu_name() -> String {
    registry_string(
        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        "ProcessorNameString",
    )
    .or_else(|| std::env::var("PROCESSOR_IDENTIFIER").ok())
    .unwrap_or_else(|| "unknown CPU".to_owned())
    .trim()
    .to_owned()
}

fn cpu_cores() -> u32 {
    // SAFETY: SYSTEM_INFO is a plain output structure and the pointer is valid.
    unsafe {
        let mut info = std::mem::zeroed::<SYSTEM_INFO>();
        GetNativeSystemInfo(&mut info);
        info.dwNumberOfProcessors.max(1)
    }
}

fn memory_mb() -> u64 {
    // SAFETY: MEMORYSTATUSEX is a plain output structure; dwLength is set first
    // as the API requires.
    unsafe {
        let mut status = std::mem::zeroed::<MEMORYSTATUSEX>();
        status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut status) != 0 {
            status.ullTotalPhys / (1024 * 1024)
        } else {
            0
        }
    }
}

fn installed_programs() -> Vec<String> {
    const ROOTS: [&str; 2] = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    let mut names = Vec::new();
    for root in ROOTS {
        let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(root, KEY_READ)
        else {
            continue;
        };
        for subkey_name in key.enum_keys().flatten() {
            let Ok(subkey) = key.open_subkey_with_flags(&subkey_name, KEY_READ) else {
                continue;
            };
            if let Ok(display_name) = subkey.get_value::<String, _>("DisplayName") {
                let display_name = display_name.trim();
                if !display_name.is_empty() {
                    names.push(display_name.to_owned());
                }
            }
        }
    }
    names
}

fn service_exists(name: &str) -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            format!(r"SYSTEM\CurrentControlSet\Services\{name}"),
            KEY_READ,
        )
        .is_ok()
}

fn running_processes() -> Vec<String> {
    let mut names = Vec::new();
    // SAFETY: the snapshot handle is checked, PROCESSENTRY32W is initialized with
    // its required dwSize, and the handle is closed before returning.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return names;
        }
        let mut entry = std::mem::zeroed::<PROCESSENTRY32W>();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let length = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..length]));
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::CheckStatus;

    #[test]
    fn firmware_virtualization_is_ok() {
        let check = check_cpu_virtualization(true, false);
        assert_eq!(check.status, CheckStatus::Ok);
    }

    #[test]
    fn active_hypervisor_masks_the_firmware_bit() {
        let check = check_cpu_virtualization(false, true);
        assert_eq!(check.status, CheckStatus::Ok);
        assert!(check.remediation.is_none());
    }

    #[test]
    fn no_virtualization_is_an_error_with_remediation() {
        let check = check_cpu_virtualization(false, false);
        assert_eq!(check.status, CheckStatus::Error);
        assert!(check.remediation.is_some());
    }
}
