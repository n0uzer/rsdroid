//! Errors produced by the ADB wrapper.

use std::path::PathBuf;
use std::time::Duration;

/// Errors produced while calling `adb.exe`.
#[derive(Debug, thiserror::Error)]
pub enum AdbError {
    /// `adb.exe` could not be started.
    #[error("cannot start adb at {path}: {source}")]
    Spawn {
        /// Path to `adb.exe`.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: std::io::Error,
    },
    /// A command did not finish in time.
    #[error("adb command timed out after {0:?}")]
    Timeout(Duration),
    /// `adb` exited unsuccessfully.
    #[error("adb exited with {status}: {stderr}")]
    Failed {
        /// Exit status, rendered for humans.
        status: String,
        /// Captured stderr.
        stderr: String,
    },
    /// `adb` printed an error but exited successfully.
    #[error("adb reported: {0}")]
    Output(String),
    /// The guest never reported boot completion.
    #[error("device did not boot within {timeout:?}: {last}")]
    BootTimeout {
        /// Overall timeout.
        timeout: Duration,
        /// Last observed value or error.
        last: String,
    },
}

impl AdbError {
    /// Whether this error means the device is not ready yet (retryable).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Spawn { .. } | Self::Timeout(_) => true,
            Self::Failed { stderr, .. } => {
                let lower = stderr.to_ascii_lowercase();
                lower.contains("device")
                    || lower.contains("offline")
                    || lower.contains("not found")
                    || lower.contains("unable")
            }
            Self::Output(message) => {
                let lower = message.to_ascii_lowercase();
                lower.contains("offline")
                    || lower.contains("not found")
                    || lower.contains("unable")
                    || lower.contains("failed")
            }
            Self::BootTimeout { .. } => false,
        }
    }
}
