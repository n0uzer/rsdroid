//! Errors produced by `rsdroid-vm`.

use std::path::PathBuf;
use std::time::Duration;

use crate::qmp::QmpError;

/// Errors produced while configuring, starting or controlling a VM.
#[derive(Debug, thiserror::Error)]
pub enum VmError {
    /// The typed configuration is not usable.
    #[error("invalid VM configuration: {0}")]
    Config(String),
    /// A filesystem or process operation failed.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Path or executable involved.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: std::io::Error,
    },
    /// QEMU exited before it could be controlled.
    #[error("QEMU exited early: {0}")]
    Exited(String),
    /// QEMU did not start listening in time.
    #[error("QEMU did not become ready within {0:?}")]
    StartupTimeout(Duration),
    /// QMP reported a failure.
    #[error(transparent)]
    Qmp(#[from] QmpError),
    /// The Windows Job Object could not be created or used.
    #[error("Job Object error: {0}")]
    Job(String),
    /// The persisted run state is missing or invalid.
    #[error("instance state error: {0}")]
    State(String),
}

impl VmError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
