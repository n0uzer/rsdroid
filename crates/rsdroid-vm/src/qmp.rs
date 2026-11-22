//! Minimal QMP client over loopback TCP.
//!
//! QMP sends a greeting, then JSON objects. Commands carry an `id`; events have
//! no `id` and are ignored while waiting for a reply. `quit` closes the
//! connection without a reply, which ADR 0001 observed and this client handles.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

/// QMP client errors.
#[derive(Debug, thiserror::Error)]
pub enum QmpError {
    /// The TCP connection failed or was closed unexpectedly.
    #[error("QMP I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// A line was not valid JSON.
    #[error("invalid QMP JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// QEMU closed the connection.
    #[error("QMP connection closed by QEMU")]
    Closed,
    /// QMP returned an error object.
    #[error("QMP command failed: {class}: {description}")]
    Command {
        /// QMP error class, such as `GenericError`.
        class: String,
        /// Human-readable QMP description.
        description: String,
    },
    /// The greeting did not look like QMP.
    #[error("unexpected QMP greeting")]
    Greeting,
}

/// Result of `query-status`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct VmStatus {
    /// QEMU's run state, such as `running`, `paused` or `shutdown`.
    pub status: String,
    /// Whether the guest CPUs are running.
    pub running: bool,
}

/// A synchronous QMP connection.
pub struct QmpClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    next_id: u64,
}

impl QmpClient {
    /// Connect to a loopback QMP port and complete the capabilities handshake.
    pub fn connect_port(port: u16, timeout: Duration) -> Result<Self, QmpError> {
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        let reader = BufReader::new(stream.try_clone()?);
        let mut client = Self {
            writer: stream,
            reader,
            next_id: 1,
        };
        client.handshake()?;
        Ok(client)
    }

    fn handshake(&mut self) -> Result<(), QmpError> {
        let greeting = self.read_message()?;
        if greeting.get("QMP").is_none() {
            return Err(QmpError::Greeting);
        }
        self.execute("qmp_capabilities", None)?;
        Ok(())
    }

    fn read_message(&mut self) -> Result<Value, QmpError> {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line)?;
        if read == 0 {
            return Err(QmpError::Closed);
        }
        Ok(serde_json::from_str(&line)?)
    }

    /// Run one QMP command and return its `return` value.
    pub fn execute(&mut self, command: &str, arguments: Option<Value>) -> Result<Value, QmpError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = json!({ "execute": command, "id": id });
        if let Some(arguments) = arguments {
            request["arguments"] = arguments;
        }
        let line = serde_json::to_string(&request)?;
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;

        loop {
            let message = self.read_message()?;
            if message.get("event").is_some() {
                continue;
            }
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let class = error
                    .get("class")
                    .and_then(Value::as_str)
                    .unwrap_or("GenericError")
                    .to_owned();
                let description = error
                    .get("desc")
                    .and_then(Value::as_str)
                    .unwrap_or("no description")
                    .to_owned();
                return Err(QmpError::Command { class, description });
            }
            return Ok(message.get("return").cloned().unwrap_or(Value::Null));
        }
    }

    /// `query-status`.
    pub fn query_status(&mut self) -> Result<VmStatus, QmpError> {
        let value = self.execute("query-status", None)?;
        Ok(serde_json::from_value(value)?)
    }

    /// Ask the guest to power down.
    pub fn system_powerdown(&mut self) -> Result<(), QmpError> {
        self.execute("system_powerdown", None).map(|_| ())
    }

    /// Reset the guest.
    pub fn system_reset(&mut self) -> Result<(), QmpError> {
        self.execute("system_reset", None).map(|_| ())
    }

    /// Pause guest execution.
    pub fn stop(&mut self) -> Result<(), QmpError> {
        self.execute("stop", None).map(|_| ())
    }

    /// Resume guest execution.
    pub fn cont(&mut self) -> Result<(), QmpError> {
        self.execute("cont", None).map(|_| ())
    }

    /// Quit QEMU. A closed connection is success because QEMU does not reply.
    pub fn quit(&mut self) -> Result<(), QmpError> {
        match self.execute("quit", None) {
            Ok(_) | Err(QmpError::Closed) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(listener: TcpListener) {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .write_all(b"{\"QMP\":{\"version\":{},\"capabilities\":[]}}\n")
                .expect("greeting");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).expect("read") == 0 {
                    break;
                }
                let request: Value = serde_json::from_str(&line).expect("json");
                let id = request.get("id").cloned().unwrap_or(Value::Null);
                let command = request
                    .get("execute")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let reply = match command {
                    "query-status" => json!({
                        "return": {"status": "running", "running": true},
                        "id": id
                    }),
                    "fail" => json!({
                        "error": {"class": "GenericError", "desc": "boom"},
                        "id": id
                    }),
                    "quit" => break,
                    _ => json!({"return": {}, "id": id}),
                };
                if command == "query-status" {
                    stream.write_all(b"{\"event\":\"RESET\"}\n").expect("event");
                }
                stream
                    .write_all(format!("{reply}\n").as_bytes())
                    .expect("reply");
            }
        });
    }

    fn test_client() -> QmpClient {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        serve_once(listener);
        QmpClient::connect_port(port, Duration::from_secs(2)).expect("connect")
    }

    #[test]
    fn handshake_query_and_control_commands_work() {
        let mut client = test_client();
        assert_eq!(client.query_status().expect("status").status, "running");
        client.stop().expect("stop");
        client.cont().expect("cont");
        client.system_powerdown().expect("powerdown");
        client.system_reset().expect("reset");
        client.quit().expect("quit");
    }

    #[test]
    fn qmp_errors_are_returned() {
        let mut client = test_client();
        let error = client.execute("fail", None).expect_err("must fail");
        assert!(error.to_string().contains("boom"));
    }
}
