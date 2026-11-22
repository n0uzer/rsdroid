//! Core types shared by the rsdroid manager, CLI, GUI and future IPC clients.
//!
//! M0 provides the environment diagnostics behind `rsdroid doctor` and the
//! per-user paths every other crate will use. Instance and manager models
//! arrive in M3.

pub mod doctor;
pub mod error;
pub mod paths;
pub mod tools;

pub use error::{Error, Result};

/// Version of the rsdroid workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
