//! Error types shared by rsdroid-core.

use std::path::PathBuf;

/// Errors produced by rsdroid-core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A filesystem operation failed.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Path that was being accessed.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: std::io::Error,
    },
    /// The per-user data directory cannot be resolved.
    #[error("cannot resolve the rsdroid data directory: {0}")]
    DataDirectory(String),
    /// The doctor bundle could not be written.
    #[error("cannot write the doctor bundle {path}: {source}")]
    Bundle {
        /// Destination of the bundle.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: std::io::Error,
    },
    /// The doctor report could not be serialized to JSON.
    #[error("cannot serialize the doctor report: {0}")]
    Json(#[from] serde_json::Error),
    /// Creating the ZIP archive failed.
    #[error("cannot create the doctor bundle: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Result alias used across rsdroid-core.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}
