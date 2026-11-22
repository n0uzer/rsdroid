//! rsdroid-adb: ADB client, port allocation and boot-completion waits.
//!
//! M1 wraps `adb.exe` only to detect boot completion. M2 writes the ADR that
//! decides between a maintained wrapper and a native ADB implementation.

mod client;
mod error;

pub use client::{is_boot_completed_value, AdbClient, DEFAULT_COMMAND_TIMEOUT};
pub use error::AdbError;
