//! rsdroid-vm: QEMU launch, QMP control, qcow2 overlays and Job Objects.
//!
//! M1 launches the pinned Android-x86 ISO live with an extracted kernel/initrd,
//! controls QEMU over loopback QMP and keeps the child in a Windows Job Object.
//! qcow2 overlays and the installed base image arrive in M3/M5.

pub mod command;
pub mod config;
mod error;
pub mod job;
pub mod process;
pub mod qmp;
pub mod state;

pub use config::{DisplayMode, VmConfig};
pub use error::VmError;
pub use process::VmProcess;
pub use qmp::{QmpClient, QmpError, VmStatus};
pub use state::{process_alive, read_state, remove_state, write_state, VmState};
