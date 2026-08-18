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
    waiters: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
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
        let (cmd, args) = split_cmd(program)?;
        let mut child = Command::new(&cmd)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
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
        acp.handshake()?;
        Ok(acp)
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

    /// The pane's view of this child: last lines, then the draft.
    pub fn grid(&self, cols: u16, rows: u16) -> crate::daemon::pane::Grid {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let draft = self.draft.lock().ok().map(|d| d.clone()).unwrap_or_default();
        let mut lines = self.lines.lock().ok().map(|l| l.clone()).unwrap_or_default();
        if self.state() == WindowState::NeedsYou {
            lines.push("needs you — y allow · n deny".into());
        }
        lines.push(format!("> {draft}"));
        while lines.len() < rows as usize {
            lines.push(String::new());
        }
        if lines.len() > rows as usize {
            let skip = lines.len() - rows as usize;
            lines = lines[skip..].to_vec();
        }
        let cursor_row = rows.saturating_sub(1);
        let cursor_col = (draft.chars().count() as u16 + 2).min(cols.saturating_sub(1));
        let runs = lines
            .iter()
            .map(|line| {
                vec![crate::daemon::pane::Run {
                    text: if line.is_empty() {
                        " ".repeat(cols as usize)
                    } else {
                        line.clone()
                    },
                    fg: None,
                    fg_rgb: None,
                    bg: None,
                    bg_rgb: None,
                    bold: false,
                    italic: false,
                    underline: false,
                    inverse: false,
                }]
            })
            .collect();
        crate::daemon::pane::Grid {
            cols,
            rows,
            cursor_col,
            cursor_row,
            lines,
            runs,
            alive: self.alive(),
            acp: true,
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
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(line.to_string());
            if lines.len() > 200 {
                let drain = lines.len() - 200;
                lines.drain(..drain);
            }
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

    fn handshake(&self) -> io::Result<()> {
        self.call(
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
        *self
            .session_id
            .lock()
            .map_err(|_| io::Error::other("acp busy"))? = Some(sid);
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
            .map_err(|_| io::Error::other(format!("acp {method} timed out")))
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
                    acp.push_line(&text);
                }
            }
            continue;
        }
        let id = msg.get("id").and_then(json_id);
        if let Some(id) = id {
            let result = msg.get("result").cloned().unwrap_or(Value::Null);
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
mod tests {
    use super::*;

    fn fake_agent() -> (tempfile::NamedTempFile, String) {
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
        send({"jsonrpc":"2.0","id":i,"result":{"protocolVersion":1,"agentCapabilities":{}}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":i,"result":{"sessionId":"s1"}})
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
}
