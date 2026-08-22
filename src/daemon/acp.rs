//! An ACP child: the daemon is the Client, the process is the Agent.
//! Stdio JSON-RPC, one object per line. State feeds the rail.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// What a window's ACP process is doing. The rail draws this.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindowState {
    #[default]
    Idle,
    Turning,
    NeedsYou,
    Dead,
}

pub struct AcpChild {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    alive: AtomicBool,
    state: Mutex<WindowState>,
    session_id: Mutex<Option<String>>,
    draft: Mutex<String>,
    next_id: AtomicU64,
    waiters: Mutex<HashMap<u64, mpsc::Sender<Result<Value, String>>>>,
    pending_prompt: Mutex<Option<u64>>,
    pending_perm: Mutex<Option<PendingPerm>>,
    lines: Mutex<Vec<String>>,
}

struct PendingPerm {
    id: Value,
    options: Vec<(String, String)>,
}

impl AcpChild {
    /// Spawn `program` (words, first is the binary) on stdio. Handshake
    /// `initialize` then `session/new` before returning.
    pub fn spawn(program: &str) -> io::Result<Arc<AcpChild>> {
        Self::spawn_resume(program, None)
    }

    /// Spawn and reopen `resume` when the child can load or resume it.
    pub fn spawn_resume(program: &str, resume: Option<&str>) -> io::Result<Arc<AcpChild>> {
        let (cmd, args) = split_cmd(program)?;
        let mut command = Command::new(&cmd);
        command.args(&args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        if let Ok(home) = std::env::var("HOME") {
            command.env("HOME", &home);
            let mut path = format!("{home}/.local/bin:{home}/.local/share/mise/shims");
            if let Ok(p) = std::env::var("PATH") {
                path = format!("{path}:{p}");
            }
            command.env("PATH", path);
        }
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("acp child has no stdout"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("acp child has no stdin"))?;
        let acp = Arc::new(AcpChild {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            alive: AtomicBool::new(true),
            state: Mutex::new(WindowState::Idle),
            session_id: Mutex::new(None),
            draft: Mutex::new(String::new()),
            next_id: AtomicU64::new(1),
            waiters: Mutex::new(HashMap::new()),
            pending_prompt: Mutex::new(None),
            pending_perm: Mutex::new(None),
            lines: Mutex::new(Vec::new()),
        });
        let pump = acp.clone();
        thread::Builder::new()
            .name("anvil-acp".into())
            .spawn(move || pump_stdout(pump, stdout))
            .map_err(io::Error::other)?;
        acp.handshake(resume)?;
        Ok(acp)
    }

    pub fn session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|s| s.clone())
    }

    pub fn state(&self) -> WindowState {
        if !self.alive() {
            return WindowState::Dead;
        }
        self.state.lock().map(|s| *s).unwrap_or(WindowState::Idle)
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// Keys into a prompt. Enter sends `session/prompt`. When the
    /// child needs you, `y` allows and `n` denies.
    pub fn write_keys(&self, data: &str) -> io::Result<()> {
        if !self.alive() {
            return Err(io::Error::other("the pane's process has ended"));
        }
        if self.state() == WindowState::NeedsYou {
            match data {
                "y" | "Y" => return self.answer_perm(true),
                "n" | "N" => return self.answer_perm(false),
                _ => {}
            }
        }
        if data == "\r" || data == "\n" {
            let text = {
                let mut draft = self.draft.lock().map_err(|_| io::Error::other("acp busy"))?;
                std::mem::take(&mut *draft)
            };
            if !text.is_empty() {
                self.push_line(&format!("> {text}"));
                self.prompt(&text)?;
            }
            return Ok(());
        }
        let mut draft = self.draft.lock().map_err(|_| io::Error::other("acp busy"))?;
        if data == "\u{7f}" {
            draft.pop();
        } else {
            draft.push_str(data);
        }
        Ok(())
    }

    /// The pane's view: transcript above, composer on the last row.
    pub fn grid(&self, cols: u16, rows: u16) -> crate::daemon::pane::Grid {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let draft = self.draft.lock().ok().map(|d| d.clone()).unwrap_or_default();
        let mut body = self.lines.lock().ok().map(|l| l.clone()).unwrap_or_default();
        if self.state() == WindowState::NeedsYou {
            body.push("needs you — y allow · n deny".into());
        }
        let width = cols.max(1) as usize;
        let mut wrapped: Vec<(String, Kind)> = Vec::new();
        if body.is_empty() {
            wrapped.push(("type a prompt, then enter".into(), Kind::Hint));
        }
        for line in &body {
            let kind = if line.starts_with("> ") {
                Kind::You
            } else {
                Kind::Agent
            };
            for row in wrap(line, width) {
                wrapped.push((row, kind));
            }
        }
        let prompt = format!("> {draft}");
        let body_rows = rows.saturating_sub(1) as usize;
        if wrapped.len() > body_rows {
            wrapped = wrapped[wrapped.len() - body_rows..].to_vec();
        }
        while wrapped.len() < body_rows {
            wrapped.push((String::new(), Kind::Hint));
        }
        wrapped.push((prompt.clone(), Kind::Composer));
        let cursor_row = rows.saturating_sub(1);
        let cursor_col = (draft.chars().count() as u16 + 2).min(cols.saturating_sub(1));
        let runs = wrapped
            .iter()
            .map(|(line, kind)| {
                let text = if line.is_empty() {
                    " ".repeat(width)
                } else {
                    line.clone()
                };
                vec![crate::daemon::pane::Run {
                    text,
                    fg: None,
                    fg_rgb: None,
                    bg: None,
                    bg_rgb: None,
                    bold: matches!(kind, Kind::Composer | Kind::You),
                    italic: matches!(kind, Kind::You | Kind::Hint),
                    underline: matches!(kind, Kind::Composer),
                    inverse: false,
                }]
            })
            .collect();
        let body: Vec<String> = wrapped.into_iter().map(|(s, _)| s).collect();
        crate::daemon::pane::Grid {
            cols,
            rows,
            cursor_col,
            cursor_row,
            lines: body,
            runs,
            alive: self.alive(),
            acp: true,
            mouse: false,
            kitty: 0,
            modify: false,
        }
    }

    pub fn prompt(&self, text: &str) -> io::Result<()> {
        let sid = self
            .session_id
            .lock()
            .map_err(|_| io::Error::other("acp busy"))?
            .clone()
            .ok_or_else(|| io::Error::other("acp has no session"))?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut state = self.state.lock().map_err(|_| io::Error::other("acp busy"))?;
            *state = WindowState::Turning;
        }
        *self
            .pending_prompt
            .lock()
            .map_err(|_| io::Error::other("acp busy"))? = Some(id);
        self.send(
            id,
            "session/prompt",
            json!({
                "sessionId": sid,
                "prompt": [{ "type": "text", "text": text }],
            }),
        )
    }

    fn push_line(&self, line: &str) {
        self.push_text(line, true);
    }

    /// Append text to the transcript. `newline` starts a fresh row;
    /// a stream chunk continues the last row.
    fn push_text(&self, text: &str, newline: bool) {
        if text.is_empty() {
            return;
        }
        let Ok(mut lines) = self.lines.lock() else {
            return;
        };
        if newline || lines.is_empty() {
            lines.push(text.to_string());
        } else if let Some(last) = lines.last_mut() {
            last.push_str(text);
        }
        const WIDTH: usize = 80;
        while let Some(last) = lines.last() {
            if last.chars().count() <= WIDTH {
                break;
            }
            let s = lines.pop().unwrap();
            let mut acc = String::new();
            let mut n = 0;
            let mut rest = String::new();
            let mut overflow = false;
            for c in s.chars() {
                if !overflow && n >= WIDTH {
                    overflow = true;
                }
                if overflow {
                    rest.push(c);
                } else {
                    acc.push(c);
                    n += 1;
                }
            }
            lines.push(acc);
            if !rest.is_empty() {
                lines.push(rest);
            }
        }
        if lines.len() > 200 {
            let drain = lines.len() - 200;
            lines.drain(..drain);
        }
    }

    fn answer_perm(&self, allow: bool) -> io::Result<()> {
        let pending = self
            .pending_perm
            .lock()
            .map_err(|_| io::Error::other("acp busy"))?
            .take();
        let Some(pending) = pending else {
            return Ok(());
        };
        let want = if allow { "allow" } else { "reject" };
        let option_id = pending
            .options
            .iter()
            .find(|(kind, _)| kind.contains(want))
            .or_else(|| pending.options.first())
            .map(|(_, id)| id.clone())
            .unwrap_or_else(|| if allow { "allow" } else { "reject" }.into());
        let result = json!({
            "outcome": { "outcome": "selected", "optionId": option_id }
        });
        self.reply(&pending.id, result)?;
        if let Ok(mut state) = self.state.lock() {
            *state = WindowState::Turning;
        }
        self.push_line(if allow { "allowed" } else { "denied" });
        Ok(())
    }

    pub fn terminate(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
        self.alive.store(false, Ordering::Relaxed);
        if let Ok(mut state) = self.state.lock() {
            *state = WindowState::Dead;
        }
    }

    pub fn hangup(&self) {
        let pid = self.child.lock().ok().map(|c| c.id());
        if let Some(pid) = pid {
            unsafe {
                libc::kill(pid as i32, libc::SIGHUP);
            }
        }
        self.alive.store(false, Ordering::Relaxed);
    }

    fn handshake(&self, resume: Option<&str>) -> io::Result<()> {
        let init = self.call(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "anvil", "version": "0.1.0" },
            }),
        )?;
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("/"))
            .display()
            .to_string();
        let caps = init.get("agentCapabilities");
        let can_load = caps
            .and_then(|c| c.get("loadSession"))
            .and_then(|v| v.as_bool())
            == Some(true);
        let can_resume = caps
            .and_then(|c| c.pointer("/sessionCapabilities/resume"))
            .is_some();
        if let Some(id) = resume.filter(|s| !s.is_empty()) {
            let params = json!({
                "sessionId": id,
                "cwd": cwd,
                "mcpServers": [],
            });
            // load replays the transcript into this pane; resume
            // reopens the same id without replay.
            if can_load && self.call("session/load", params.clone()).is_ok() {
                self.set_session_id(id)?;
                return Ok(());
            }
            if can_resume && self.call("session/resume", params).is_ok() {
                self.set_session_id(id)?;
                return Ok(());
            }
        }
        let result = self.call(
            "session/new",
            json!({
                "cwd": cwd,
                "mcpServers": [],
            }),
        )?;
        let sid = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| io::Error::other("session/new had no sessionId"))?
            .to_string();
        self.set_session_id(sid)
    }

    fn set_session_id(&self, sid: impl Into<String>) -> io::Result<()> {
        *self
            .session_id
            .lock()
            .map_err(|_| io::Error::other("acp busy"))? = Some(sid.into());
        Ok(())
    }

    fn call(&self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.waiters
            .lock()
            .map_err(|_| io::Error::other("acp busy"))?
            .insert(id, tx);
        self.send(id, method, params)?;
        rx.recv_timeout(Duration::from_secs(20))
            .map_err(|_| io::Error::other(format!("acp {method} timed out")))?
            .map_err(|e| io::Error::other(format!("acp {method}: {e}")))
    }

    fn reply(&self, id: &Value, result: Value) -> io::Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        let mut stdin = self.stdin.lock().map_err(|_| io::Error::other("acp busy"))?;
        writeln!(stdin, "{msg}")?;
        stdin.flush()
    }

    fn send(&self, id: u64, method: &str, params: Value) -> io::Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut stdin = self.stdin.lock().map_err(|_| io::Error::other("acp busy"))?;
        writeln!(stdin, "{msg}")?;
        stdin.flush()
    }
}

#[derive(Clone, Copy)]
enum Kind {
    You,
    Agent,
    Composer,
    Hint,
}

fn wrap(s: &str, cols: usize) -> Vec<String> {
    if cols == 0 {
        return vec![String::new()];
    }
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut acc = String::new();
    let mut n = 0;
    for c in s.chars() {
        if n >= cols {
            rows.push(std::mem::take(&mut acc));
            n = 0;
        }
        acc.push(c);
        n += 1;
    }
    if !acc.is_empty() {
        rows.push(acc);
    }
    rows
}

fn pump_stdout(acp: Arc<AcpChild>, stdout: impl std::io::Read) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
            if method == "session/request_permission" {
                let options = perm_options(&msg);
                if let Ok(mut pending) = acp.pending_perm.lock() {
                    *pending = msg.get("id").cloned().map(|id| PendingPerm { id, options });
                }
                if let Ok(mut state) = acp.state.lock() {
                    *state = WindowState::NeedsYou;
                }
                acp.push_line("needs you");
            } else if method == "session/update" {
                if let Some(text) = update_text(&msg) {
                    let newline = acp
                        .lines
                        .lock()
                        .ok()
                        .and_then(|l| l.last().cloned())
                        .is_some_and(|s| s.starts_with("> "));
                    acp.push_text(&text, newline);
                }
            }
            continue;
        }
        let id = msg.get("id").and_then(json_id);
        if let Some(id) = id {
            let result = if let Some(err) = msg.get("error") {
                Err(err.to_string())
            } else {
                Ok(msg.get("result").cloned().unwrap_or(Value::Null))
            };
            if result.is_ok() {
                if let Ok(mut pending) = acp.pending_prompt.lock() {
                    if *pending == Some(id) {
                        *pending = None;
                        if let Ok(mut state) = acp.state.lock() {
                            if *state == WindowState::Turning {
                                *state = WindowState::Idle;
                            }
                        }
                    }
                }
            }
            if let Ok(mut waiters) = acp.waiters.lock() {
                if let Some(tx) = waiters.remove(&id) {
                    let _ = tx.send(result);
                }
            }
        }
    }
    acp.alive.store(false, Ordering::Relaxed);
    if let Ok(mut state) = acp.state.lock() {
        *state = WindowState::Dead;
    }
}

fn perm_options(msg: &Value) -> Vec<(String, String)> {
    msg.get("params")
        .and_then(|p| p.get("options"))
        .and_then(|o| o.as_array())
        .map(|opts| {
            opts.iter()
                .filter_map(|o| {
                    let id = o.get("optionId")?.as_str()?.to_string();
                    let kind = o
                        .get("kind")
                        .and_then(|k| k.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some((kind, id))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn update_text(msg: &Value) -> Option<String> {
    let update = msg.get("params")?.get("update")?;
    let content = update.get("content")?;
    if let Some(text) = content.get("text").and_then(|t| t.as_str()) {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        return Some(t.to_string());
    }
    None
}

fn json_id(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn split_cmd(program: &str) -> io::Result<(String, Vec<String>)> {
    let mut parts = program.split_whitespace();
    let cmd = parts
        .next()
        .ok_or_else(|| io::Error::other("acp program is empty"))?
        .to_string();
    Ok((cmd, parts.map(str::to_string).collect()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn fake_agent() -> (tempfile::NamedTempFile, String) {
        let mut file = tempfile::Builder::new().suffix(".py").tempfile().unwrap();
        file.write_all(
            br#"
import json, sys
def recv():
    line = sys.stdin.readline()
    if not line:
        return None
    return json.loads(line)
def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
while True:
    m = recv()
    if m is None:
        break
    method = m.get("method")
    i = m.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":i,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":True,"sessionCapabilities":{"resume":{}}}}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":i,"result":{"sessionId":"s1"}})
    elif method == "session/load" or method == "session/resume":
        sid = m.get("params",{}).get("sessionId","")
        send({"jsonrpc":"2.0","id":i,"result":{"sessionId":sid}})
    elif method == "session/prompt":
        text = ""
        for block in m.get("params",{}).get("prompt",[]):
            if block.get("type") == "text":
                text += block.get("text","")
        if text == "ask":
            send({"jsonrpc":"2.0","id":99,"method":"session/request_permission","params":{
                "sessionId":"s1",
                "options":[
                    {"optionId":"allow-once","name":"Allow","kind":"allow_once"},
                    {"optionId":"reject-once","name":"Reject","kind":"reject_once"}
                ]
            }})
        else:
            send({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"}}}})
            send({"jsonrpc":"2.0","id":i,"result":{"stopReason":"end_turn"}})
"#,
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().display().to_string();
        (file, path)
    }

    #[test]
    fn handshake_then_prompt_returns_idle() {
        let (_keep, path) = fake_agent();
        let acp = AcpChild::spawn(&format!("python3 {path}")).unwrap();
        assert_eq!(acp.state(), WindowState::Idle);
        acp.prompt("hello").unwrap();
        for _ in 0..50 {
            if acp.state() == WindowState::Idle {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(acp.state(), WindowState::Idle);
        let grid = acp.grid(40, 10);
        assert_eq!(grid.cursor_row, 9);
        assert!(grid.lines.last().unwrap().starts_with("> "), "{:?}", grid.lines.last());
        assert!(grid.lines.iter().any(|l| l.contains("ok")), "{:?}", grid.lines);
    }

    #[test]
    fn composer_sits_on_the_last_row() {
        let (_keep, path) = fake_agent();
        let acp = AcpChild::spawn(&format!("python3 {path}")).unwrap();
        acp.write_keys("hi").unwrap();
        let grid = acp.grid(20, 8);
        assert_eq!(grid.lines.len(), 8);
        assert_eq!(grid.lines[7], "> hi");
        assert_eq!(grid.cursor_row, 7);
        assert_eq!(grid.cursor_col, 4);
    }

    #[test]
    fn prompt_ask_sets_needs_you() {
        let (_keep, path) = fake_agent();
        let acp = AcpChild::spawn(&format!("python3 {path}")).unwrap();
        acp.prompt("ask").unwrap();
        for _ in 0..50 {
            if acp.state() == WindowState::NeedsYou {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(acp.state(), WindowState::NeedsYou);
        acp.write_keys("y").unwrap();
        for _ in 0..50 {
            if acp.state() == WindowState::Turning {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(acp.state(), WindowState::Turning);
    }

    #[test]
    fn spawn_reopens_the_named_session() {
        let (_keep, path) = fake_agent();
        let acp = AcpChild::spawn_resume(&format!("python3 {path}"), Some("ses_pane_1")).unwrap();
        assert_eq!(acp.session_id().as_deref(), Some("ses_pane_1"));
    }

    #[test]
    fn missing_session_falls_back_to_new() {
        let mut file = tempfile::Builder::new().suffix(".py").tempfile().unwrap();
        file.write_all(
            br#"
import json, sys
def recv():
    line = sys.stdin.readline()
    if not line:
        return None
    return json.loads(line)
def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
while True:
    m = recv()
    if m is None:
        break
    method = m.get("method")
    i = m.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":i,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":True}}})
    elif method == "session/load":
        send({"jsonrpc":"2.0","id":i,"error":{"code":-32000,"message":"gone"}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":i,"result":{"sessionId":"fresh"}})
"#,
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().display().to_string();
        let acp = AcpChild::spawn_resume(&format!("python3 {path}"), Some("gone")).unwrap();
        assert_eq!(acp.session_id().as_deref(), Some("fresh"));
    }
}
