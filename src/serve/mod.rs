//! `anvil serve` — owns hammers. Casings attach over a unix socket.

mod client;
mod inspect;
mod mount;
mod proto;

pub use client::Client;
pub use inspect::{Fiber, Report, Service, Slot};

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::ask::{self, AskSink, HttpCompleter};
use crate::config::Config;
use crate::frame::{EventBody, FrameRoot};
use crate::{Anvil, StrikeReply};

use proto::{Msg, Req};

pub struct ServeOpts {
    pub root: PathBuf,
    pub hammer: PathBuf,
    pub config: Option<PathBuf>,
    pub sock: PathBuf,
}

pub fn default_sock() -> PathBuf {
    if let Ok(p) = std::env::var("ANVIL_SOCK") {
        return PathBuf::from(p);
    }
    runtime_dir().join("anvil.sock")
}

fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(format!("/tmp/anvil-{}", uid()))
}

fn uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(1000)
}

fn pid_path(sock: &Path) -> PathBuf {
    sock.with_extension("pid")
}

pub fn run(opts: ServeOpts) -> io::Result<()> {
    let sock = opts.sock.clone();
    let state = State::open(&opts)?;
    listen(sock, state)
}

pub(crate) struct State {
    pub(crate) root: FrameRoot,
    hammer: PathBuf,
    cfg: Option<Config>,
    pub(crate) slots: Mutex<HashMap<String, Arc<Mutex<Anvil>>>>,
    running: AtomicBool,
    pub(crate) sock: PathBuf,
    last_active: Mutex<Option<String>>,
    pub(crate) mounts: Arc<mount::Mounts>,
}

impl State {
    fn open(opts: &ServeOpts) -> io::Result<Self> {
        let root = FrameRoot::open(&opts.root).map_err(io::Error::other)?;
        root.ensure_defaults().map_err(io::Error::other)?;
        let cfg = match opts.config.as_deref() {
            Some(p) => Some(Config::load_from(p).map_err(io::Error::other)?.1),
            None => Config::load().ok().map(|(_, c)| c),
        };
        Ok(Self {
            root,
            hammer: opts.hammer.clone(),
            cfg,
            slots: Mutex::new(HashMap::new()),
            running: AtomicBool::new(true),
            sock: opts.sock.clone(),
            last_active: Mutex::new(None),
            mounts: Arc::new(mount::Mounts::default()),
        })
    }

    pub(crate) fn touch(&self, session: &str) {
        if let Ok(mut g) = self.last_active.lock() {
            *g = Some(session.to_string());
        }
    }

    pub(crate) fn live_front(&self) -> Option<String> {
        self.last_active
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .or_else(|| {
                self.root
                    .layout("default")
                    .ok()
                    .and_then(|l| l.front_session)
            })
    }

    fn slot(&self, session: &str) -> io::Result<Arc<Mutex<Anvil>>> {
        self.touch(session);
        if !self.root.session_exists(session) {
            self.root
                .create_session(session)
                .map_err(io::Error::other)?;
        }
        let mut slots = self.slots.lock().map_err(|_| io::Error::other("slots"))?;
        if let Some(slot) = slots.get(session) {
            return Ok(slot.clone());
        }
        let anvil =
            Anvil::open(self.root.session_dir(session), &self.hammer).map_err(io::Error::other)?;
        let slot = Arc::new(Mutex::new(anvil));
        slots.insert(session.to_string(), slot.clone());
        drop(slots);
        let _ = self.root.append_event(
            session,
            EventBody::Fiber {
                state: "hot".into(),
            },
        );
        Ok(slot)
    }
}

fn listen(sock: PathBuf, state: State) -> io::Result<()> {
    if let Some(parent) = sock.parent() {
        fs::create_dir_all(parent)?;
    }
    reclaim_stale(&sock);
    let listener = UnixListener::bind(&sock)?;
    listener.set_nonblocking(true)?;
    fs::write(pid_path(&sock), format!("{}\n", std::process::id()))?;
    eprintln!("anvil serve {}", sock.display());
    let state = Arc::new(state);
    while state.running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = state.clone();
                thread::Builder::new()
                    .name("anvil-conn".into())
                    .spawn(move || {
                        if let Err(err) = handle_conn(stream, &state) {
                            if err.kind() != io::ErrorKind::UnexpectedEof {
                                eprintln!("anvil serve conn: {err}");
                            }
                        }
                    })?;
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(err) => {
                let _ = fs::remove_file(&sock);
                let _ = fs::remove_file(pid_path(&sock));
                return Err(err);
            }
        }
    }
    let _ = fs::remove_file(&sock);
    let _ = fs::remove_file(pid_path(&sock));
    Ok(())
}

fn reclaim_stale(sock: &Path) {
    if !sock.exists() {
        return;
    }
    if UnixStream::connect(sock).is_ok() {
        return;
    }
    let _ = fs::remove_file(sock);
    let _ = fs::remove_file(pid_path(sock));
}

fn handle_conn(stream: UnixStream, state: &State) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(());
        }
        let req: Req = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(err) => {
                write_msg(
                    &mut writer,
                    &Msg::Error {
                        id: "0".into(),
                        text: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        if matches!(req, Req::Shutdown { .. }) {
            write_msg(
                &mut writer,
                &Msg::Bye {
                    id: req.id().into(),
                },
            )?;
            state.running.store(false, Ordering::Relaxed);
            return Ok(());
        }
        dispatch(&req, state, &mut writer)?;
    }
}

fn dispatch(req: &Req, state: &State, writer: &mut UnixStream) -> io::Result<()> {
    match req {
        Req::Ping { id } => write_msg(writer, &Msg::Pong { id: id.clone() }),
        Req::Shutdown { .. } => Ok(()),
        Req::Expose { id, session } => {
            state.touch(session);
            write_msg(writer, &Msg::Pong { id: id.clone() })
        }
        Req::Mount { id, kind, slot } => {
            let slot = slot
                .clone()
                .unwrap_or_else(|| inspect::SLOT_STATUS.to_string());
            match state.mounts.mount(kind, &slot) {
                Ok(mount_id) => write_msg(
                    writer,
                    &Msg::Mounted {
                        id: id.clone(),
                        mount_id,
                        mount_kind: kind.clone(),
                        slot,
                    },
                ),
                Err(text) => write_msg(
                    writer,
                    &Msg::Error {
                        id: id.clone(),
                        text,
                    },
                ),
            }
        }
        Req::Unmount { id, mount_id } => match state.mounts.unmount(mount_id) {
            Ok(_) => write_msg(
                writer,
                &Msg::Unmounted {
                    id: id.clone(),
                    mount_id: mount_id.clone(),
                },
            ),
            Err(text) => write_msg(
                writer,
                &Msg::Error {
                    id: id.clone(),
                    text,
                },
            ),
        },
        Req::Inspect { id } => write_msg(
            writer,
            &Msg::Inspect {
                id: id.clone(),
                report: state.inspect(),
            },
        ),
        Req::Strike { id, session, code } => {
            let start = Instant::now();
            match with_session(state, session, |anvil| anvil.strike(code)) {
                Ok(reply) => {
                    let ms = start.elapsed().as_millis() as u64;
                    let _ = state.root.append_event(
                        session,
                        EventBody::Strike {
                            code: code.clone(),
                            stdout: reply.stdout.clone(),
                            stderr: reply.stderr.clone(),
                            error: reply.error.clone(),
                            ok: reply.ok,
                            ms: Some(ms),
                        },
                    );
                    write_msg(
                        writer,
                        &Msg::Reply {
                            id: id.clone(),
                            reply,
                        },
                    )
                }
                Err(err) => write_msg(
                    writer,
                    &Msg::Error {
                        id: id.clone(),
                        text: err.to_string(),
                    },
                ),
            }
        }
        Req::Reset { id, session } => match with_session(state, session, |anvil| anvil.reset()) {
            Ok(reply) => write_msg(
                writer,
                &Msg::Reply {
                    id: id.clone(),
                    reply,
                },
            ),
            Err(err) => write_msg(
                writer,
                &Msg::Error {
                    id: id.clone(),
                    text: err.to_string(),
                },
            ),
        },
        Req::Ask {
            id,
            session,
            prompt,
            provider,
            model,
        } => run_ask(
            state,
            writer,
            id,
            session,
            prompt,
            provider.as_deref(),
            model.as_deref(),
        ),
    }
}

fn with_session<T>(
    state: &State,
    session: &str,
    f: impl FnOnce(&mut Anvil) -> Result<T, crate::AnvilError>,
) -> io::Result<T> {
    let slot = state.slot(session)?;
    let mut anvil = slot.lock().map_err(|_| io::Error::other("session busy"))?;
    f(&mut anvil).map_err(io::Error::other)
}

struct WireSink<'a> {
    writer: &'a mut UnixStream,
    root: &'a FrameRoot,
    id: String,
    session: String,
}

impl AskSink for WireSink<'_> {
    fn on_status(&mut self, status: &str) {
        let _ = write_msg(
            self.writer,
            &Msg::Status {
                id: self.id.clone(),
                session: self.session.clone(),
                text: status.into(),
            },
        );
    }
    fn on_draft(&mut self, text: &str) {
        if ask::extract_python(text).is_none() {
            let _ = self
                .root
                .append_event(&self.session, EventBody::Thinking { text: text.into() });
        }
        let _ = write_msg(
            self.writer,
            &Msg::Draft {
                id: self.id.clone(),
                session: self.session.clone(),
                text: text.into(),
            },
        );
    }
    fn on_strike(&mut self, code: &str, reply: &StrikeReply) {
        let _ = self.root.append_event(
            &self.session,
            EventBody::Strike {
                code: code.into(),
                stdout: reply.stdout.clone(),
                stderr: reply.stderr.clone(),
                error: reply.error.clone(),
                ok: reply.ok,
                ms: None,
            },
        );
        let _ = write_msg(
            self.writer,
            &Msg::Strike {
                id: self.id.clone(),
                session: self.session.clone(),
                code: code.into(),
                stdout: reply.stdout.clone(),
                stderr: reply.stderr.clone(),
                error: reply.error.clone(),
                ok: reply.ok,
            },
        );
    }
}

fn run_ask(
    state: &State,
    writer: &mut UnixStream,
    id: &str,
    session: &str,
    prompt: &str,
    provider: Option<&str>,
    model: Option<&str>,
) -> io::Result<()> {
    let Some(cfg) = state.cfg.as_ref() else {
        return write_msg(
            writer,
            &Msg::Error {
                id: id.into(),
                text: "serve has no provider config; ask needs ~/.config/anvil/config.yaml".into(),
            },
        );
    };
    let (_name, prov) = match cfg.provider(provider) {
        Ok(v) => v,
        Err(err) => {
            return write_msg(
                writer,
                &Msg::Error {
                    id: id.into(),
                    text: err.to_string(),
                },
            );
        }
    };
    let Some(model) = cfg.model_for(prov, model) else {
        return write_msg(
            writer,
            &Msg::Error {
                id: id.into(),
                text: "no model: set default_model or pass --model".into(),
            },
        );
    };
    let mut llm = HttpCompleter {
        provider: prov.clone(),
        model: model.clone(),
    };
    let _ = state.root.append_event(
        session,
        EventBody::Ask {
            prompt: prompt.into(),
            provider: provider.map(str::to_string),
            model: Some(model),
        },
    );
    let slot = state.slot(session)?;
    let mut anvil = slot.lock().map_err(|_| io::Error::other("session busy"))?;
    let mut sink = WireSink {
        writer,
        root: &state.root,
        id: id.into(),
        session: session.into(),
    };
    let log = state.root.load_events(session).unwrap_or_default();
    match ask::ask_with_log(&mut llm, &mut anvil, prompt, &log, &mut sink) {
        Ok(result) => {
            let _ = state.root.append_event(
                session,
                EventBody::Answer {
                    text: result.answer.clone(),
                },
            );
            write_msg(
                writer,
                &Msg::Answer {
                    id: id.into(),
                    session: session.into(),
                    text: result.answer,
                },
            )
        }
        Err(err) => write_msg(
            writer,
            &Msg::Error {
                id: id.into(),
                text: err.to_string(),
            },
        ),
    }
}

fn write_msg(stream: &mut UnixStream, msg: &Msg) -> io::Result<()> {
    let line = serde_json::to_string(msg).expect("msg is serializable");
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

pub struct Spawn {
    pub root: PathBuf,
    pub hammer: PathBuf,
    pub config: Option<PathBuf>,
    pub sock: PathBuf,
}

/// Connect if serve is up; otherwise start `anvil serve` and wait.
pub fn connect_or_spawn(spawn: &Spawn) -> io::Result<Client> {
    if let Ok(mut c) = Client::connect(&spawn.sock) {
        if c.ping().is_ok() {
            return Ok(c);
        }
    }
    start_daemon(spawn)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut last = io::Error::other("serve did not come up");
    while Instant::now() < deadline {
        match Client::connect(&spawn.sock) {
            Ok(mut c) => {
                if c.ping().is_ok() {
                    return Ok(c);
                }
            }
            Err(err) => last = err,
        }
        thread::sleep(Duration::from_millis(40));
    }
    Err(last)
}

pub fn stop(sock: &Path) -> io::Result<()> {
    match Client::connect(sock) {
        Ok(mut c) => c.shutdown(),
        Err(err)
            if err.kind() == io::ErrorKind::ConnectionRefused
                || err.kind() == io::ErrorKind::NotFound =>
        {
            reclaim_stale(sock);
            Ok(())
        }
        Err(err) => Err(err),
    }
}

pub fn status(sock: &Path) -> io::Result<bool> {
    match Client::connect(sock) {
        Ok(mut c) => Ok(c.ping().is_ok()),
        Err(_) => Ok(false),
    }
}

fn start_daemon(spawn: &Spawn) -> io::Result<()> {
    let bin = sibling_anvil();
    if let Some(parent) = spawn.sock.parent() {
        fs::create_dir_all(parent)?;
    }
    let log_path = spawn.root.join("serve.log");
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let err = log.try_clone()?;
    let mut cmd = Command::new(&bin);
    cmd.arg("serve")
        .arg("--root")
        .arg(&spawn.root)
        .arg("--hammer")
        .arg(&spawn.hammer)
        .arg("--sock")
        .arg(&spawn.sock)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .process_group(0);
    if let Some(cfg) = &spawn.config {
        cmd.arg("--config").arg(cfg);
    }
    let child = cmd.spawn().map_err(|err| {
        io::Error::other(format!("failed to spawn {} serve: {err}", bin.display()))
    })?;
    // Detached: do not kill when smith closes.
    std::mem::forget(child);
    Ok(())
}

fn sibling_anvil() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("anvil");
            if cand.is_file() {
                return cand;
            }
        }
    }
    PathBuf::from("anvil")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_hammer;
    use tempfile::TempDir;

    fn boot() -> (TempDir, PathBuf, thread::JoinHandle<io::Result<()>>) {
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("anvil.sock");
        let root = tmp.path().join("root");
        let opts = ServeOpts {
            root: root.clone(),
            hammer: default_hammer(),
            config: None,
            sock: sock.clone(),
        };
        let handle = thread::Builder::new()
            .name("anvil-serve-test".into())
            .spawn(move || run(opts))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if Client::connect(&sock).map(|mut c| c.ping()).is_ok() {
                return (tmp, sock, handle);
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("serve did not bind {sock:?}");
    }

    #[test]
    fn strike_persists_after_client_disconnects() {
        let (_tmp, sock, handle) = boot();
        {
            let mut c = Client::connect(&sock).unwrap();
            let reply = c.strike("fox", "x = 21").unwrap();
            assert!(reply.ok, "{reply:?}");
        }
        {
            let mut c = Client::connect(&sock).unwrap();
            let reply = c.strike("fox", "x * 2").unwrap();
            assert!(reply.ok, "{reply:?}");
            assert_eq!(reply.value, serde_json::json!(42));
            c.shutdown().unwrap();
        }
        let _ = handle.join();
    }

    #[test]
    fn strike_is_logged_and_inspect_sees_hot_fiber() {
        let (tmp, sock, handle) = boot();
        let mut c = Client::connect(&sock).unwrap();
        c.strike("fox", "1+1").unwrap();
        let report = c.inspect().unwrap();
        let fox = report
            .services
            .iter()
            .find(|s| s.name == "fox")
            .expect("fox service");
        assert_eq!(fox.state, "hot");
        assert!(fox.events >= 1, "{report:?}");
        let transcript = report
            .slots
            .iter()
            .find(|s| s.name == "session.transcript")
            .expect("transcript slot");
        assert_eq!(transcript.occupant.as_deref(), Some("fox"));
        let main = report
            .slots
            .iter()
            .find(|s| s.name == "casing.main")
            .expect("main slot");
        assert_eq!(main.occupant.as_deref(), Some("fox"));
        let root = FrameRoot::open(tmp.path().join("root")).unwrap();
        let events = root.load_events("fox").unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e.body, EventBody::Strike { ok: true, .. })),
            "{events:?}"
        );
        c.shutdown().unwrap();
        let _ = handle.join();
    }

    #[test]
    fn expose_moves_slots_without_warming() {
        let (_tmp, sock, handle) = boot();
        let mut c = Client::connect(&sock).unwrap();
        c.expose("research").unwrap();
        let report = c.inspect().unwrap();
        let transcript = report
            .slots
            .iter()
            .find(|s| s.name == "session.transcript")
            .unwrap();
        assert_eq!(transcript.occupant.as_deref(), Some("research"));
        let research = report.services.iter().find(|s| s.name == "research");
        assert!(research.is_none() || research.map(|s| s.state.as_str()) == Some("cold"));
        c.shutdown().unwrap();
        let _ = handle.join();
    }

    #[test]
    fn mount_clock_occupies_status_and_unmount_clears_it() {
        let (_tmp, sock, handle) = boot();
        let mut c = Client::connect(&sock).unwrap();
        let (id, slot) = c.mount("clock", None).unwrap();
        assert_eq!(slot, "casing.status");
        assert_eq!(id, "dyn-1");
        let report = c.inspect().unwrap();
        let status = report
            .slots
            .iter()
            .find(|s| s.name == "casing.status")
            .unwrap();
        assert_eq!(status.occupant.as_deref(), Some("dyn-1"));
        assert!(status.text.as_ref().is_some_and(|t| t.contains(':')));
        assert!(report.fibers.iter().any(|f| f.name == "mount/dyn-1"));
        c.unmount(&id).unwrap();
        let report = c.inspect().unwrap();
        let status = report
            .slots
            .iter()
            .find(|s| s.name == "casing.status")
            .unwrap();
        assert!(status.occupant.is_none());
        assert!(status.text.is_none());
        assert!(!report.fibers.iter().any(|f| f.name == "mount/dyn-1"));
        c.shutdown().unwrap();
        let _ = handle.join();
    }

    #[test]
    fn two_sessions_are_independent() {
        let (_tmp, sock, handle) = boot();
        let mut c = Client::connect(&sock).unwrap();
        c.strike("a", "n = 1").unwrap();
        c.strike("b", "n = 2").unwrap();
        assert_eq!(c.strike("a", "n").unwrap().value, serde_json::json!(1));
        assert_eq!(c.strike("b", "n").unwrap().value, serde_json::json!(2));
        c.shutdown().unwrap();
        let _ = handle.join();
    }
}
