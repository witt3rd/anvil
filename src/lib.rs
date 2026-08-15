//! anvil — the harness. Spawns a hammer, issues strikes, respawns on death.

mod protocol;

pub mod ask;
pub mod catalog;
pub mod complete;
pub mod config;
pub mod oauth;
pub mod secret;
pub mod tui;

pub use protocol::{Op, StrikeReply, StrikeRequest};

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnvilError {
    #[error("hammer is not executable at {}", .0.display())]
    MissingHammer(PathBuf),
    #[error("failed to spawn hammer: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("hammer stdin closed")]
    StdinClosed,
    #[error("hammer stdout closed")]
    StdoutClosed,
    #[error("hammer returned invalid JSON: {0}")]
    BadReply(#[source] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Anvil {
    store: PathBuf,
    hammer_path: PathBuf,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    next_id: AtomicU64,
}

impl Anvil {
    pub fn open(
        store: impl Into<PathBuf>,
        hammer_path: impl Into<PathBuf>,
    ) -> Result<Self, AnvilError> {
        let store = store.into();
        let hammer_path = hammer_path.into();
        if !hammer_path.is_file() {
            return Err(AnvilError::MissingHammer(hammer_path));
        }
        std::fs::create_dir_all(&store)?;
        let mut anvil = Self {
            store,
            hammer_path,
            child: None,
            stdin: None,
            stdout: None,
            next_id: AtomicU64::new(1),
        };
        anvil.ensure_hammer()?;
        Ok(anvil)
    }

    pub fn store(&self) -> &Path {
        &self.store
    }

    pub fn hammer_alive(&mut self) -> bool {
        match &mut self.child {
            Some(child) => child.try_wait().ok().flatten().is_none(),
            None => false,
        }
    }

    pub fn strike(&mut self, code: &str) -> Result<StrikeReply, AnvilError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        self.request(StrikeRequest::strike(id, code))
    }

    pub fn ping(&mut self) -> Result<StrikeReply, AnvilError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        self.request(StrikeRequest::ping(id))
    }

    pub fn reset(&mut self) -> Result<StrikeReply, AnvilError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        self.request(StrikeRequest::reset(id))
    }

    pub fn request(&mut self, req: StrikeRequest) -> Result<StrikeReply, AnvilError> {
        self.ensure_hammer()?;
        match self.write_read(&req) {
            Ok(reply) => Ok(reply),
            Err(err) => {
                // One retry on a dead guest: hang another hammer, replay.
                self.reap();
                self.ensure_hammer()?;
                self.write_read(&req).map_err(|_| err)
            }
        }
    }

    fn write_read(&mut self, req: &StrikeRequest) -> Result<StrikeReply, AnvilError> {
        let stdin = self.stdin.as_mut().ok_or(AnvilError::StdinClosed)?;
        let line = serde_json::to_string(req).expect("request is always valid JSON");
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;

        let stdout = self.stdout.as_mut().ok_or(AnvilError::StdoutClosed)?;
        let mut reply = String::new();
        let n = stdout.read_line(&mut reply)?;
        if n == 0 {
            return Err(AnvilError::StdoutClosed);
        }
        serde_json::from_str(reply.trim()).map_err(AnvilError::BadReply)
    }

    fn ensure_hammer(&mut self) -> Result<(), AnvilError> {
        if self.hammer_alive() {
            return Ok(());
        }
        self.reap();
        let mut child = Command::new("python3")
            .arg(&self.hammer_path)
            .env("ANVIL_STORE", &self.store)
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(AnvilError::Spawn)?;
        let stdin = child.stdin.take().ok_or(AnvilError::StdinClosed)?;
        let stdout = child.stdout.take().ok_or(AnvilError::StdoutClosed)?;
        self.stdin = Some(stdin);
        self.stdout = Some(BufReader::new(stdout));
        self.child = Some(child);
        // Prove the child is on the protocol before we hand it to a caller.
        let ready = StrikeRequest::ping("0");
        self.write_read(&ready)?;
        Ok(())
    }

    fn reap(&mut self) {
        self.stdin = None;
        self.stdout = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Anvil {
    fn drop(&mut self) {
        let alive = self.hammer_alive();
        if alive {
            if let Some(stdin) = self.stdin.as_mut() {
                let line = serde_json::to_string(&StrikeRequest::shutdown("0")).unwrap_or_default();
                let _ = writeln!(stdin, "{line}");
                let _ = stdin.flush();
            }
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.wait_timeout();
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

trait WaitTimeout {
    fn wait_timeout(&mut self) -> std::io::Result<()>;
}

impl WaitTimeout for Child {
    fn wait_timeout(&mut self) -> std::io::Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        loop {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

pub fn default_store() -> PathBuf {
    if let Ok(dir) = std::env::var("ANVIL_STORE") {
        return PathBuf::from(dir);
    }
    dirs_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".anvil")
        .join("default")
}

pub fn default_hammer() -> PathBuf {
    if let Ok(path) = std::env::var("ANVIL_HAMMER") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("hammer/hammer.py")
}

pub(crate) fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn harness() -> (TempDir, Anvil) {
        let tmp = TempDir::new().unwrap();
        let anvil = Anvil::open(tmp.path(), default_hammer()).unwrap();
        (tmp, anvil)
    }

    #[test]
    fn strike_returns_last_expression() {
        let (_tmp, mut anvil) = harness();
        let reply = anvil.strike("2 + 2").unwrap();
        assert!(reply.ok, "{reply:?}");
        assert_eq!(reply.value, serde_json::json!(4));
    }

    #[test]
    fn print_is_stdout_and_value_is_null() {
        let (_tmp, mut anvil) = harness();
        let reply = anvil.strike("print('hi')").unwrap();
        assert!(reply.ok, "{reply:?}");
        assert_eq!(reply.stdout, "hi\n");
        assert!(reply.value.is_null());
    }

    #[test]
    fn namespace_persists_across_strikes() {
        let (_tmp, mut anvil) = harness();
        anvil.strike("x = 21").unwrap();
        let reply = anvil.strike("x * 2").unwrap();
        assert_eq!(reply.value, serde_json::json!(42));
    }

    #[test]
    fn persist_across_respawn() {
        let (_tmp, mut anvil) = harness();
        anvil.strike("x = 1").unwrap();
        anvil.reap();
        let reply = anvil.strike("x").unwrap();
        assert!(reply.ok, "{reply:?}");
        assert_eq!(reply.value, serde_json::json!(1));
    }

    #[test]
    fn syntax_and_runtime_errors_are_replies_not_crashes() {
        let (_tmp, mut anvil) = harness();
        let boom = anvil.strike("1/0").unwrap();
        assert!(!boom.ok);
        assert!(
            boom.error
                .as_deref()
                .unwrap_or("")
                .contains("ZeroDivisionError"),
            "{boom:?}"
        );
        let still = anvil.strike("3").unwrap();
        assert!(still.ok);
        assert_eq!(still.value, serde_json::json!(3));
    }
}
