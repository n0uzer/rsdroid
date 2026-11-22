//! Thin wrapper around `adb.exe` used for M1 boot detection.
//!
//! M2 writes an ADR that decides between keeping this wrapper and implementing a
//! native ADB client. Nothing outside this module depends on the choice yet.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

use crate::error::AdbError;

/// Default timeout for one `adb` invocation.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(20);

/// Client for one loopback ADB serial.
#[derive(Debug, Clone)]
pub struct AdbClient {
    adb: PathBuf,
    serial: String,
    command_timeout: Duration,
}

impl AdbClient {
    /// Address one VM through its forwarded loopback port.
    pub fn new(adb: PathBuf, adb_port: u16) -> Self {
        Self {
            adb,
            serial: format!("127.0.0.1:{adb_port}"),
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    /// Override the per-command timeout.
    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Serial string passed to `adb -s`.
    pub fn serial(&self) -> &str {
        &self.serial
    }

    /// Path to the `adb.exe` in use.
    pub fn adb(&self) -> &PathBuf {
        &self.adb
    }

    /// Run `adb connect` for this serial.
    pub fn connect(&self) -> Result<String, AdbError> {
        let output = self.run(&["connect", &self.serial])?;
        if output.starts_with("connected to") || output.starts_with("already connected to") {
            Ok(output)
        } else {
            Err(AdbError::Output(output))
        }
    }

    /// Disconnect this serial.
    pub fn disconnect(&self) -> Result<(), AdbError> {
        self.run(&["disconnect", &self.serial]).map(|_| ())
    }

    /// Read one Android property through the device shell.
    pub fn getprop(&self, name: &str) -> Result<String, AdbError> {
        self.run(&["-s", &self.serial, "shell", "getprop", name])
    }

    /// Whether `sys.boot_completed` is `1`.
    pub fn is_boot_completed(&self) -> Result<bool, AdbError> {
        Ok(is_boot_completed_value(
            &self.getprop("sys.boot_completed")?,
        ))
    }

    /// Poll until the guest reports `sys.boot_completed=1`.
    ///
    /// Returns the time spent waiting. Retryable ADB failures (device offline,
    /// adbd not listening yet) are retried until the overall deadline.
    pub fn wait_for_boot(&self, timeout: Duration, poll: Duration) -> Result<Duration, AdbError> {
        let started = Instant::now();
        let deadline = started + timeout;
        let mut connected = false;
        let mut last = "no response from adb".to_owned();
        loop {
            if !connected {
                match self.connect() {
                    Ok(_) => connected = true,
                    Err(error) => last = error.to_string(),
                }
            }
            if connected {
                match self.getprop("sys.boot_completed") {
                    Ok(value) if is_boot_completed_value(&value) => {
                        return Ok(started.elapsed());
                    }
                    Ok(value) => last = format!("sys.boot_completed={value:?}"),
                    Err(error) => {
                        last = error.to_string();
                        connected = false;
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(AdbError::BootTimeout { timeout, last });
            }
            std::thread::sleep(poll);
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, AdbError> {
        let mut command = Command::new(&self.adb);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|source| AdbError::Spawn {
            path: self.adb.clone(),
            source,
        })?;
        let status = match child.wait_timeout(self.command_timeout) {
            Ok(Some(status)) => status,
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AdbError::Timeout(self.command_timeout));
            }
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AdbError::Spawn {
                    path: self.adb.clone(),
                    source,
                });
            }
        };

        let mut stdout = String::new();
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_string(&mut stdout)
                .map_err(|source| AdbError::Spawn {
                    path: self.adb.clone(),
                    source,
                })?;
        }
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr)
                .map_err(|source| AdbError::Spawn {
                    path: self.adb.clone(),
                    source,
                })?;
        }
        if !status.success() {
            return Err(AdbError::Failed {
                status: status.to_string(),
                stderr: stderr.trim().to_owned(),
            });
        }
        let stdout = stdout.trim().to_owned();
        if stdout.is_empty() && !stderr.trim().is_empty() {
            return Err(AdbError::Failed {
                status: status.to_string(),
                stderr: stderr.trim().to_owned(),
            });
        }
        Ok(stdout)
    }
}

/// Parse the value printed by `getprop sys.boot_completed`.
pub fn is_boot_completed_value(value: &str) -> bool {
    value.trim() == "1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_value_is_strict() {
        assert!(is_boot_completed_value("1\n"));
        assert!(is_boot_completed_value(" 1 "));
        for value in ["", "0", "true", "1 1"] {
            assert!(!is_boot_completed_value(value), "{value:?}");
        }
    }

    #[test]
    fn serial_uses_loopback_port() {
        let client = AdbClient::new(PathBuf::from(r"C:\adb\adb.exe"), 15555);
        assert_eq!(client.serial(), "127.0.0.1:15555");
        assert_eq!(client.adb(), &PathBuf::from(r"C:\adb\adb.exe"));
    }
}
