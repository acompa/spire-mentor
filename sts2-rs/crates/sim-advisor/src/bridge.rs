//! The TCP client for the Spire Mentor mod's bridge (was
//! `realgame_bridge.RealGameBridgeClient`): newline-delimited UTF-8 JSON
//! both ways. The mod sends every message (advice requests, human actions,
//! rewind broadcasts, run ends); the advisor answers advice requests and
//! sends the coach's records and the control plane's commands.
//!
//! A reader thread moves complete lines into a queue, so the loop can ask
//! whether a newer request is already waiting ([`Link::newer_advice_request_pending`])
//! without touching the socket other threads write to.

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;
use sts2_advisor::control::Link;

#[derive(Debug, Default)]
struct Queue {
    lines: VecDeque<String>,
    closed: bool,
}

#[derive(Debug, Default)]
struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Why no message came.
#[derive(Debug)]
pub enum RecvError {
    /// The mod closed the connection (or it failed).
    Closed,
    /// Nothing arrived within the idle timeout.
    Timeout,
}

impl std::fmt::Display for RecvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => f.write_str("Connection lost while waiting for state"),
            Self::Timeout => f.write_str("No data received within the idle timeout"),
        }
    }
}

/// The last ids a message carried, echoed on the next send.
#[derive(Debug, Default)]
struct Echo {
    request_id: Option<Value>,
    state_id: Option<Value>,
}

/// One connection to the mod.
#[derive(Debug)]
pub struct Bridge {
    shared: Arc<Shared>,
    writer: Mutex<TcpStream>,
    echo: Mutex<Echo>,
}

impl Bridge {
    /// Connect, retrying `attempts` times `delay` apart (the game may not
    /// have opened its server yet). `warn` gets each failed attempt.
    ///
    /// # Errors
    ///
    /// Every attempt failed.
    pub fn connect(
        host: &str,
        port: u16,
        attempts: u32,
        delay: Duration,
        warn: &dyn Fn(&str),
    ) -> std::io::Result<Arc<Self>> {
        let mut last = None;
        for attempt in 1..=attempts {
            match TcpStream::connect((host, port)) {
                Ok(stream) => return Self::from_stream(stream),
                Err(e) => {
                    warn(&format!(
                        "Connection attempt {attempt}/{attempts} to {host}:{port} failed: {e}"
                    ));
                    last = Some(e);
                    if attempt < attempts {
                        std::thread::sleep(delay);
                    }
                }
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            format!(
                "Could not connect to real-game bridge at {host}:{port} after {attempts} \
                 attempts: {}",
                last.map_or_else(String::new, |e| e.to_string())
            ),
        ))
    }

    /// A bridge over an open stream; its reader thread starts here.
    ///
    /// # Errors
    ///
    /// The stream cannot be cloned for the reader.
    pub fn from_stream(stream: TcpStream) -> std::io::Result<Arc<Self>> {
        let _ = stream.set_nodelay(true);
        let mut reader = stream.try_clone()?;
        let shared = Arc::new(Shared::default());
        let sh = shared.clone();
        std::thread::Builder::new()
            .name("bridge-reader".into())
            .spawn(move || {
                let mut buf: Vec<u8> = Vec::new();
                let mut chunk = vec![0u8; 65536];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            let mut lines = Vec::new();
                            while let Some(i) = buf.iter().position(|b| *b == b'\n') {
                                let line: Vec<u8> = buf.drain(..=i).collect();
                                lines.push(
                                    String::from_utf8_lossy(&line[..line.len() - 1])
                                        .trim()
                                        .to_string(),
                                );
                            }
                            if !lines.is_empty() {
                                sh.lock().lines.extend(lines);
                                sh.ready.notify_all();
                            }
                        }
                    }
                }
                sh.lock().closed = true;
                sh.ready.notify_all();
            })?;
        Ok(Arc::new(Self {
            shared,
            writer: Mutex::new(stream),
            echo: Mutex::new(Echo::default()),
        }))
    }

    /// The next message, waiting up to `idle`. Blank and malformed lines
    /// are skipped (`warn` gets the malformed ones). The message's
    /// `request_id` / `state_id` are remembered for the next send.
    ///
    /// # Errors
    ///
    /// The connection closed, or nothing came within `idle`.
    pub fn receive(&self, idle: Duration, warn: &dyn Fn(&str)) -> Result<Value, RecvError> {
        loop {
            let line = {
                let q = self.shared.lock();
                let (mut q, timeout) = self
                    .shared
                    .ready
                    .wait_timeout_while(q, idle, |q| q.lines.is_empty() && !q.closed)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match q.lines.pop_front() {
                    Some(line) => line,
                    None if q.closed => return Err(RecvError::Closed),
                    None if timeout.timed_out() => return Err(RecvError::Timeout),
                    None => continue,
                }
            };
            if line.is_empty() {
                continue;
            }
            let msg: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    let raw: String = line.chars().take(200).collect();
                    warn(&format!("Invalid JSON from bridge: {e} (raw: {raw})"));
                    continue;
                }
            };
            if let Some(m) = msg.as_object() {
                let mut echo = self
                    .echo
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(id) = m.get("request_id") {
                    echo.request_id = Some(id.clone());
                }
                if let Some(id) = m.get("state_id") {
                    echo.state_id = Some(id.clone());
                }
            }
            return Ok(msg);
        }
    }

    /// Close the connection (the reader thread sees EOF and ends).
    pub fn close(&self) {
        let w = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = w.shutdown(std::net::Shutdown::Both);
    }
}

impl Link for Bridge {
    fn send(&self, msg: &Value, echo_state_id: bool) -> std::io::Result<()> {
        let mut payload = msg.clone();
        if let Some(m) = payload.as_object_mut() {
            let echo = self
                .echo
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !m.contains_key("request_id") {
                if let Some(id) = echo.request_id.as_ref().filter(|v| !v.is_null()) {
                    m.insert("request_id".into(), id.clone());
                }
            }
            if echo_state_id && !m.contains_key("state_id") {
                if let Some(id) = echo.state_id.as_ref().filter(|v| !v.is_null()) {
                    m.insert("state_id".into(), id.clone());
                }
            }
        }
        let mut line = serde_json::to_string(&payload).map_err(std::io::Error::other)?;
        line.push('\n');
        let mut w = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        w.write_all(line.as_bytes())?;
        w.flush()
    }

    fn newer_advice_request_pending(&self, advice_for: &Value) -> bool {
        let q = self.shared.lock();
        q.lines.iter().any(|raw| {
            raw.contains("\"advice_request\"")
                && serde_json::from_str::<Value>(raw).is_ok_and(|v| {
                    v.get("type").and_then(Value::as_str) == Some("advice_request")
                        && sts2_advisor::py::eq(sts2_advisor::py::get(&v, "advice_for"), advice_for)
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead as _;
    use std::net::TcpListener;

    #[test]
    fn a_request_is_answered_with_its_ids_echoed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mod_side = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(b"{\"type\":\"advice_request\",\"advice_for\":\"map_select\",\"request_id\":\"r1\",\"state_id\":7}\nnot json\n\n")
                .unwrap();
            s.write_all(b"{\"type\":\"advice_request\",\"advice_for\":\"map_select\",\"request_id\":\"r2\"}\n")
                .unwrap();
            let mut line = String::new();
            std::io::BufReader::new(s.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            line
        });
        let bridge =
            Bridge::connect("127.0.0.1", port, 3, Duration::from_millis(50), &|_| {}).unwrap();
        let warned = Mutex::new(Vec::new());
        let first = bridge
            .receive(Duration::from_secs(5), &|w| {
                warned.lock().unwrap().push(w.to_string());
            })
            .unwrap();
        assert_eq!(first["request_id"], "r1");
        // The second request may still be in flight; wait for it to queue.
        for _ in 0..100 {
            if bridge.newer_advice_request_pending(&Value::from("map_select")) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(bridge.newer_advice_request_pending(&Value::from("map_select")));
        assert!(!bridge.newer_advice_request_pending(&Value::from("card_reward")));
        bridge
            .send(&serde_json::json!({"action": "advice"}), true)
            .unwrap();
        let sent: Value = serde_json::from_str(&mod_side.join().unwrap()).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({"action": "advice", "request_id": "r1", "state_id": 7})
        );
        let second = bridge
            .receive(Duration::from_secs(5), &|w| {
                warned.lock().unwrap().push(w.to_string());
            })
            .unwrap();
        assert_eq!(second["request_id"], "r2");
        assert_eq!(warned.lock().unwrap().len(), 1, "the malformed line");
        drop(bridge);
    }
}
