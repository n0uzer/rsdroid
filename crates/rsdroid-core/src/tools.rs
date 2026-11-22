//! Locate external tools used by rsdroid.
//!
//! The search order is shared by `doctor`, `run` and `stop` so all three agree on
//! which QEMU and adb they use.

use std::path::{Path, PathBuf};

/// Name of the QEMU x86_64 system emulator executable.
pub const QEMU_EXE: &str = "qemu-system-x86_64.exe";
/// Name of the Android Debug Bridge executable.
pub const ADB_EXE: &str = "adb.exe";

/// Find `qemu-system-x86_64.exe`.
///
/// Precedence: the explicit path, `RSDROID_QEMU`, `PATH`, the pinned installer's
/// default directory, then `%LOCALAPPDATA%\rsdroid\tools\qemu`.
pub fn find_qemu(explicit: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = explicit {
        candidates.push(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os("RSDROID_QEMU") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(path) = find_on_path(QEMU_EXE) {
        candidates.push(path);
    }
    candidates.push(PathBuf::from(r"C:\Program Files\qemu").join(QEMU_EXE));
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local)
                .join(r"rsdroid\tools\qemu")
                .join(QEMU_EXE),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// Find `adb.exe`.
///
/// Precedence: the explicit path, `PATH`, `ANDROID_SDK_ROOT`, `ANDROID_HOME`,
/// `%LOCALAPPDATA%\Android\Sdk\platform-tools`, then `C:\Android\platform-tools`.
pub fn find_adb(explicit: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = explicit {
        candidates.push(path.to_path_buf());
    }
    if let Some(path) = find_on_path(ADB_EXE) {
        candidates.push(path);
    }
    for variable in ["ANDROID_SDK_ROOT", "ANDROID_HOME"] {
        if let Some(root) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(root).join(r"platform-tools").join(ADB_EXE));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local)
                .join(r"Android\Sdk\platform-tools")
                .join(ADB_EXE),
        );
    }
    candidates.push(PathBuf::from(r"C:\Android\platform-tools").join(ADB_EXE));
    candidates.into_iter().find(|path| path.is_file())
}

fn find_on_path(executable: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(executable))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_missing_path_falls_back_to_a_real_tool() {
        let missing = Path::new(r"C:\definitely\missing\qemu-system-x86_64.exe");
        assert_ne!(find_qemu(Some(missing)), Some(missing.to_path_buf()));
    }
}
