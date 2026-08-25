//! Watch an HTTP door for rail state, and send a prompt through it.
//! Paths come from the catalog (`HttpDoor`), not from a brand name.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::catalog::HttpDoor;
use super::acp::WindowState;

/// Polls `base` (`http://127.0.0.1:port`) for session status.
pub struct HttpWatch {
    base: String,
    spec: HttpDoor,
    directory: Option<String>,
    state: Mutex<WindowState>,
    activity: Mutex<Option<String>>,
    session_id: Mutex<Option<String>>,
    stop: AtomicBool,
}

impl HttpWatch {
    pub fn start(base: &str, spec: HttpDoor, directory: Option<String>) -> Arc<HttpWatch> {
        let watch = Arc::new(HttpWatch {
            base: base.to_string(),
            spec,
            directory,
            state: Mutex::new(WindowState::Idle),
            activity: Mutex::new(None),
            session_id: Mutex::new(None),
            stop: AtomicBool::new(false),
        });
        let pump = watch.clone();
        let _ = thread::Builder::new()
            .name("anvil-watch".into())
            .spawn(move || pump.run());
        watch
    }

    /// A prompt into the TUI's current session. Prefers the TUI
    /// composer so the operator sees it; falls back to the session
    /// message door.
    pub fn prompt(&self, text: &str) -> io::Result<()> {
        let append = serde_json::to_string(&json!({ "text": text }))
            .map_err(io::Error::other)?;
        if let Some(path) = self.spec.append.as_deref() {
            if http_post(&self.base, path, &append).is_ok() {
                if let Some(submit) = self.spec.submit.as_deref() {
                    let _ = http_post(&self.base, submit, "{}");
                }
                return Ok(());
            }
        }
        let sessions = self
            .spec
            .sessions
            .as_deref()
            .ok_or_else(|| io::Error::other("this door has no sessions path"))?;
        let list = http_get(&self.base, sessions)?;
        let sid = current_work_session(&list, self.directory.as_deref())
            .map(|(id, _)| id)
            .ok_or_else(|| io::Error::other("the agent has no session"))?;
        let body = serde_json::to_string(&json!({
            "parts": [{ "type": "text", "text": text }]
        }))
        .map_err(io::Error::other)?;
        let path = self
            .spec
            .prompt
            .as_deref()
            .unwrap_or("")
            .replace("{id}", &sid);
        if path.is_empty() {
            return Err(io::Error::other("this door has no prompt path"));
        }
        http_post(&self.base, &path, &body)?;
        Ok(())
    }

    pub fn state(&self) -> WindowState {
        self.state.lock().map(|s| *s).unwrap_or(WindowState::Idle)
    }

    pub fn activity(&self) -> Option<String> {
        self.activity.lock().ok().and_then(|a| a.clone())
    }

    pub fn session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|s| s.clone())
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn run(&self) {
        let base = self.base.as_str();
        for _ in 0..50 {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            if let Some(health) = self.spec.health.as_deref() {
                if http_get(base, health).is_ok() {
                    break;
                }
            } else {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let mut ticks = 0u32;
        while !self.stop.load(Ordering::Relaxed) {
            let next = match self.spec.status.as_deref() {
                Some(status) => match http_get(base, status) {
                    Ok(body) => status_from_json(&body),
                    Err(_) => WindowState::Idle,
                },
                None => WindowState::Idle,
            };
            if let Ok(mut state) = self.state.lock() {
                *state = next;
            }
            ticks = ticks.wrapping_add(1);
            if ticks % 5 == 1 {
                if let Some((id, act)) = refresh_work(base, &self.spec, self.directory.as_deref()) {
                    if let Ok(mut slot) = self.session_id.lock() {
                        *slot = Some(id);
                    }
                    if let Some(act) = act {
                        if let Ok(mut slot) = self.activity.lock() {
                            *slot = Some(act);
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(400));
        }
    }
}

/// Map a status JSON blob onto a rail mark.
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
    http_exchange(base, "GET", path, None, Duration::from_millis(400))
}

fn http_post(base: &str, path: &str, body: &str) -> io::Result<String> {
    http_exchange(base, "POST", path, Some(body), Duration::from_secs(2))
}

fn http_exchange(
    base: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> io::Result<String> {
    let (host, port) = parse_base(base)?;
    let mut stream = TcpStream::connect((host.as_str(), port))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    match body {
        Some(body) => write!(
            stream,
            "{method} {path} HTTP/1.0\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )?,
        None => write!(
            stream,
            "{method} {path} HTTP/1.0\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
        )?,
    }
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    let status = buf
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("");
    if !status.starts_with('2') && !status.is_empty() {
        return Err(io::Error::other(format!("agent door {status}")));
    }
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or(&buf);
    Ok(body.to_string())
}

/// Newest session on `GET /session`.
pub fn current_session_id(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let sessions = match &v {
        Value::Array(a) => a.as_slice(),
        Value::Object(m) => m.get("sessions")?.as_array()?.as_slice(),
        _ => return None,
    };
    let mut best: Option<(u64, String)> = None;
    for s in sessions {
        let id = s.get("id")?.as_str()?.to_string();
        let t = s
            .pointer("/time/updated")
            .or_else(|| s.pointer("/time/created"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        if best.as_ref().is_none_or(|(bt, _)| t >= *bt) {
            best = Some((t, id));
        }
    }
    best.map(|(_, id)| id)
}

fn refresh_work(
    base: &str,
    spec: &HttpDoor,
    directory: Option<&str>,
) -> Option<(String, Option<String>)> {
    let sessions = spec.sessions.as_deref()?;
    let list = http_get(base, sessions).ok()?;
    let (id, title) = current_work_session(&list, directory)?;
    if !placeholder_title(&title) {
        return Some((id, Some(terse(&title, 28))));
    }
    let path = spec.messages.as_deref()?.replace("{id}", &id);
    let act = http_get(base, &path)
        .ok()
        .and_then(|msgs| last_user_line(&msgs).map(|s| terse(&s, 28)));
    Some((id, act))
}

/// Newest session that is real work, not a courier fork.
/// When `directory` is set, prefer sessions in that project.
pub fn current_work_session(body: &str, directory: Option<&str>) -> Option<(String, String)> {
    pick_work_session(body, directory).or_else(|| {
        if directory.is_some() {
            pick_work_session(body, None)
        } else {
            None
        }
    })
}

fn pick_work_session(body: &str, directory: Option<&str>) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(body).ok()?;
    let sessions = match &v {
        Value::Array(a) => a.as_slice(),
        Value::Object(m) => m.get("sessions")?.as_array()?.as_slice(),
        _ => return None,
    };
    let mut best: Option<(u64, String, String)> = None;
    let mut fallback: Option<(u64, String, String)> = None;
    for s in sessions {
        if let Some(want) = directory {
            let dir = s.get("directory").and_then(|d| d.as_str()).unwrap_or("");
            if dir != want {
                continue;
            }
        }
        let id = s.get("id")?.as_str()?.to_string();
        let title = s
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let t = s
            .pointer("/time/updated")
            .or_else(|| s.pointer("/time/created"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        if title.starts_with("anvil-courier") {
            continue;
        }
        let row = (t, id, title);
        if placeholder_title(&row.2) {
            if fallback.as_ref().is_none_or(|(bt, ..)| t >= *bt) {
                fallback = Some(row);
            }
        } else if best.as_ref().is_none_or(|(bt, ..)| t >= *bt) {
            best = Some(row);
        }
    }
    best.or(fallback).map(|(_, id, title)| (id, title))
}

fn placeholder_title(title: &str) -> bool {
    title.is_empty() || title.starts_with("New session")
}

/// Last user text in a `/session/:id/message` payload.
pub fn last_user_line(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let items = match &v {
        Value::Array(a) => a.as_slice(),
        Value::Object(m) => m.get("messages")?.as_array()?.as_slice(),
        _ => return None,
    };
    for item in items.iter().rev() {
        let role = item
            .pointer("/info/role")
            .or_else(|| item.get("role"))
            .and_then(|r| r.as_str())
            .unwrap_or("");
        if role != "user" {
            continue;
        }
        let parts = item
            .get("parts")
            .and_then(|p| p.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        for p in parts.iter().rev() {
            if p.get("type").and_then(|t| t.as_str()) != Some("text") {
                continue;
            }
            let text = p.get("text").and_then(|t| t.as_str()).unwrap_or("").trim();
            if !text.is_empty() {
                return Some(text.replace('\n', " "));
            }
        }
    }
    None
}

fn terse(s: &str, max: usize) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        return s;
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
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

    #[test]
    fn newest_session_wins() {
        let body = r#"[{"id":"old","time":{"updated":1}},{"id":"new","time":{"updated":9}}]"#;
        assert_eq!(current_session_id(body).as_deref(), Some("new"));
        let wrapped = r#"{"sessions":[{"id":"only"}]}"#;
        assert_eq!(current_session_id(wrapped).as_deref(), Some("only"));
        assert_eq!(current_session_id("[]"), None);
    }

    #[test]
    fn work_session_skips_courier_and_placeholders() {
        let body = r#"[
            {"id":"c","title":"anvil-courier-silent","time":{"updated":30}},
            {"id":"n","title":"New session - 2026","time":{"updated":20}},
            {"id":"w","title":"RunPod RTX 6000 pricing","time":{"updated":10}}
        ]"#;
        let (id, title) = current_work_session(body, None).unwrap();
        assert_eq!(id, "w");
        assert_eq!(title, "RunPod RTX 6000 pricing");
    }

    #[test]
    fn work_session_prefers_this_directory() {
        let body = r#"[
            {"id":"other","title":"Elsewhere","directory":"/x","time":{"updated":90}},
            {"id":"here","title":"This pane","directory":"/home/dt/src/witt3rd/anvil","time":{"updated":10}}
        ]"#;
        let (id, title) = current_work_session(body, Some("/home/dt/src/witt3rd/anvil")).unwrap();
        assert_eq!(id, "here");
        assert_eq!(title, "This pane");
    }

    #[test]
    fn last_user_line_reads_the_transcript() {
        let body = r#"[{"info":{"role":"user"},"parts":[{"type":"text","text":"what's the pricing on runpod"}]},{"info":{"role":"assistant"},"parts":[{"type":"text","text":"$0.74/hr"}]}]"#;
        assert_eq!(
            last_user_line(body).as_deref(),
            Some("what's the pricing on runpod")
        );
    }
}
