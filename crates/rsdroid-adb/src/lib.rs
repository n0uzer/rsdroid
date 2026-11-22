//! rsdroid-adb: ADB client, port allocation and boot-completion waits.
//!
//! M0 only reserves the crate boundary. Implementation starts in M2 after the
//! ADR that chooses between wrapping `adb.exe` and a native client.
