//! QEMU process ownership.

use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::command::build_arguments;
use crate::config::VmConfig;
use crate::error::VmError;
use crate::job::JobObject;
use crate::qmp::QmpClient;

/// A running QEMU process contained in a kill-on-close Job Object.
pub struct VmProcess {
    child: Child,
    qemu: PathBuf,
    _job: JobObject,
}

impl VmProcess {
    /// Validate the configuration and start QEMU.
    pub fn start(config: &VmConfig) -> Result<Self, VmError> {
        config.validate()?;
        if let Some(parent) = config.log_file.parent() {
            fs::create_dir_all(parent).map_err(|error| VmError::io(parent, error))?;
        }
        if let Some(parent) = config.pid_file.parent() {
            fs::create_dir_all(parent).map_err(|error| VmError::io(parent, error))?;
        }
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.log_file)
            .map_err(|error| VmError::io(&config.log_file, error))?;
        let log_error = log
            .try_clone()
            .map_err(|error| VmError::io(&config.log_file, error))?;

        let mut command = Command::new(&config.qemu);
        command
            .args(build_arguments(config))
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_error));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command
            .spawn()
            .map_err(|error| VmError::io(&config.qemu, error))?;
        let job = match JobObject::new() {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(VmError::Job(error.to_string()));
            }
        };
        if let Err(error) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(VmError::Job(error.to_string()));
        }
        Ok(Self {
            child,
            qemu: config.qemu.clone(),
            _job: job,
        })
    }

    /// PID of QEMU.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Non-blocking exit check.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, VmError> {
        self.child
            .try_wait()
            .map_err(|error| VmError::io(&self.qemu, error))
    }

    /// Wait for QEMU's QMP socket, failing early if QEMU exits.
    pub fn connect_qmp(&mut self, port: u16, timeout: Duration) -> Result<QmpClient, VmError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_wait()? {
                return Err(VmError::Exited(format!(
                    "QEMU exited with {status} before QMP was ready; see the log"
                )));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(VmError::StartupTimeout(timeout));
            }
            let attempt = remaining.min(Duration::from_millis(250));
            match QmpClient::connect_port(port, attempt) {
                Ok(client) => return Ok(client),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    /// Wait up to `timeout` for QEMU to exit.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<Option<ExitStatus>, VmError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Forcefully terminate QEMU.
    pub fn kill(&mut self) -> Result<(), VmError> {
        self.child
            .kill()
            .map_err(|error| VmError::io(&self.qemu, error))
    }

    /// Block until QEMU exits.
    pub fn wait(&mut self) -> Result<ExitStatus, VmError> {
        self.child
            .wait()
            .map_err(|error| VmError::io(&self.qemu, error))
    }
}
