//! Per-user paths. Nothing outside the returned directories is ever touched.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::{Error, Result};

const APP_DIR: &str = "rsdroid";
const DATA_DIR_ENV: &str = "RSDROID_DATA_DIR";

/// Resolve the rsdroid data directory.
///
/// Precedence: `RSDROID_DATA_DIR`, then `%LOCALAPPDATA%\rsdroid` as required by
/// `AGENTS.md`.
pub fn data_dir() -> Result<PathBuf> {
    data_dir_from(
        std::env::var_os(DATA_DIR_ENV),
        std::env::var_os("LOCALAPPDATA"),
    )
}

/// Logs directory inside the rsdroid data directory.
pub fn logs_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("logs"))
}

/// Directory for pinned tools such as QEMU and scrcpy-server.
pub fn tools_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("tools"))
}

/// Directory for base images and instance overlays.
pub fn images_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("images"))
}

fn data_dir_from(
    override_dir: Option<OsString>,
    local_app_data: Option<OsString>,
) -> Result<PathBuf> {
    if let Some(dir) = override_dir.filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let base = local_app_data
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::DataDirectory("LOCALAPPDATA is not set".to_owned()))?;
    Ok(PathBuf::from(base).join(APP_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_override_wins() {
        let result = data_dir_from(
            Some(OsString::from(r"C:\custom\rsdroid")),
            Some(OsString::from(r"C:\Users\test\AppData\Local")),
        )
        .unwrap();
        assert_eq!(result, PathBuf::from(r"C:\custom\rsdroid"));
    }

    #[test]
    fn empty_override_is_ignored() {
        let result = data_dir_from(
            Some(OsString::new()),
            Some(OsString::from(r"C:\Users\test\AppData\Local")),
        )
        .unwrap();
        assert_eq!(
            result,
            PathBuf::from(r"C:\Users\test\AppData\Local").join("rsdroid")
        );
    }

    #[test]
    fn missing_local_app_data_is_an_error() {
        let result = data_dir_from(None, None);
        assert!(matches!(result, Err(Error::DataDirectory(_))));
    }
}
