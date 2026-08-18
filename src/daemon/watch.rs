//! Watch an agent's HTTP door (OpenCode's server) for rail state.
//! The TUI owns the PTY; this only reads.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::acp::WindowState;

/// Polls `base` (`http://127.0.0.1:port`) for session status.
pub struct HttpWatch {
    state: Mutex<WindowState>,
    stop: AtomicBool,
}

impl HttpWatch {
    pub fn start(base: &str) -> Arc<HttpWatch> {
        let watch = Arc::new(HttpWatch {
            state: Mutex::new(WindowState::Idle),
            stop: AtomicBool::new(false),
        });
        let pump = watch.clone();
        let base = base.to_string();
        let _ = thread::Builder::new()
            .name("anvil-watch".into())
            .spawn(move || pump.run(&base));
        watch
    }

    pub fn state(&self) -> WindowState {
        self.state.lock().map(|s| *s).unwrap_or(WindowState::Idle)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn run(&self, base: &str) {
        for _ in 0..50 {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            if http_get(base, "/global/health").is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        while !self.stop.load(Ordering::Relaxed) {
            let next = match http_get(base, "/session/status") {
                Ok(body) => status_from_json(&body),
                Err(_) => WindowState::Idle,
            };
            if let Ok(mut state) = self.state.lock() {
                *state = next;
            }
            thread::sleep(Duration::from_millis(400));
        }
    }
}

/// Map OpenCode `/session/status` JSON onto a rail mark.
pub fn status_from_json(body: &str) -> WindowState {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return WindowState::Idle;
    };
    let mut turning = false;
    let mut needs = false;
    visit(&v, &mut turning, &mut needs);
    if needs {
        WindowState::NeedsYou
    } else if turning {
        WindowState::Turning
    } else {
        WindowState::Idle
    }
}

fn visit(v: &Value, turning: &mut bool, needs: &mut bool) {
    match v {
        Value::String(s) => classify(s, turning, needs),
        Value::Object(map) => {
            if let Some(Value::String(t)) = map.get("type") {
                classify(t, turning, needs);
            }
            if let Some(Value::String(s)) = map.get("status") {
                classify(s, turning, needs);
            }
            for val in map.values() {
                visit(val, turning, needs);
            }
        }
        Value::Array(items) => {
            for item in items {
                visit(item, turning, needs);
            }
        }
        _ => {}
    }
}

fn classify(s: &str, turning: &mut bool, needs: &mut bool) {
    let s = s.to_ascii_lowercase();
    if s.contains("permission") || s.contains("question") || s.contains("ask") {
        *needs = true;
    }
    if s == "busy" || s == "retry" || s == "running" || s.contains("busy") {
        *turning = true;
    }
}

fn http_get(base: &str, path: &str) -> io::Result<String> {
    let (host, port) = parse_base(base)?;
    let mut stream = TcpStream::connect((host.as_str(), port))?;
    stream.set_read_timeout(Some(Duration::from_millis(400)))?;
    stream.set_write_timeout(Some(Duration::from_millis(400)))?;
    write!(
        stream,
        "GET {path} HTTP/1.0\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or(&buf);
    Ok(body.to_string())
}

fn parse_base(base: &str) -> io::Result<(String, u16)> {
    let rest = base
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::other("watch is http://host:port"))?;
    let (host, port) = rest
        .split_once(':')
        .ok_or_else(|| io::Error::other("watch needs a port"))?;
    let port = port
        .trim_end_matches('/')
        .parse()
        .map_err(|_| io::Error::other("watch port"))?;
    Ok((host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_is_turning() {
        assert_eq!(
            status_from_json(r#"{"abc":{"type":"busy"}}"#),
            WindowState::Turning
        );
    }

    #[test]
    fn permission_is_needs_you() {
        assert_eq!(
            status_from_json(r#"{"abc":{"type":"permission"}}"#),
            WindowState::NeedsYou
        );
    }

    #[test]
    fn idle_stays_idle() {
        assert_eq!(
            status_from_json(r#"{"abc":{"type":"idle"}}"#),
            WindowState::Idle
        );
    }
}
