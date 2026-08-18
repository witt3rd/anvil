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

    /// Keys into a prompt. Enter sends `session/prompt`.
    pub fn write_keys(&self, data: &str) -> io::Result<()> {
        if !self.alive() {
            return Err(io::Error::other("the pane's process has ended"));
        }
        if data == "\r" || data == "\n" {
            let text = {
                let mut draft = self.draft.lock().map_err(|_| io::Error::other("acp busy"))?;
                std::mem::take(&mut *draft)
            };
            if !text.is_empty() {
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
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
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
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| io::Error::other(format!("acp {method} timed out")))
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
                if let Ok(mut state) = acp.state.lock() {
                    *state = WindowState::NeedsYou;
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
            send({"jsonrpc":"2.0","id":99,"method":"session/request_permission","params":{"sessionId":"s1"}})
        else:
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
    }
}
