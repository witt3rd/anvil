//! The daemon: stays up, owns sessions, serves clients over a unix
//! socket. One JSON object per line in, one per line out. EOF is a
//! detach — the sessions, windows, and panes stay.

pub mod pane;
pub mod session;

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::proto::{Reply, Request, Value};
use session::{Session, Sessions};

/// `$XDG_RUNTIME_DIR/anvil.sock`, or `/tmp/anvil-<uid>/anvil.sock`.
/// `ANVIL_SOCK` overrides.
pub fn default_sock() -> PathBuf {
    if let Ok(p) = std::env::var("ANVIL_SOCK") {
        return PathBuf::from(p);
    }
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("anvil.sock");
    }
    PathBuf::from(format!("/tmp/anvil-{}/anvil.sock", uid()))
}

fn uid() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(0)
}

/// The daemon is a command of `anvil`. Runs until it is stopped.
pub fn run(root: PathBuf, sock: PathBuf) -> io::Result<()> {
    let sessions = Arc::new(Sessions::open(root)?);
    if let Some(parent) = sock.parent() {
        fs::create_dir_all(parent)?;
    }
    reclaim_stale(&sock);
    let listener = UnixListener::bind(&sock)?;
    fs::write(pid_path(&sock), format!("{}\n", std::process::id()))?;
    eprintln!("anvil daemon {}", sock.display());
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let sessions = sessions.clone();
                thread::spawn(move || {
                    let _ = serve_client(stream, sessions);
                });
            }
            Err(err) => eprintln!("anvil daemon: accept: {err}"),
        }
    }
    let _ = fs::remove_file(&sock);
    let _ = fs::remove_file(pid_path(&sock));
    Ok(())
}

fn pid_path(sock: &Path) -> PathBuf {
    sock.with_extension("pid")
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

/// One client connection. Each request gets one reply. EOF is a
/// detach: the client drops; the sessions, windows, and panes stay.
fn serve_client(stream: UnixStream, sessions: Arc<Sessions>) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut attached: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(());
        }
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(request) => dispatch(request, &sessions, &mut attached),
            Err(err) => Reply::err("", format!("cannot read the request: {err}")),
        };
        let mut out = serde_json::to_string(&reply)?;
        out.push('\n');
        writer.write_all(out.as_bytes())?;
        writer.flush()?;
    }
}

fn dispatch(
    request: Request,
    sessions: &Sessions,
    attached: &mut Option<String>,
) -> Reply {
    let id = request.id().to_string();
    match handle(request, sessions, attached) {
        Ok(value) => Reply::ok(&id, value),
        Err(err) => Reply::err(&id, err.to_string()),
    }
}

fn handle(request: Request, sessions: &Sessions, attached: &mut Option<String>) -> io::Result<Value> {
    match request {
        Request::Enumerate { .. } => Ok(Value::Sessions {
            sessions: sessions.list(),
        }),
        Request::Create {
            session, window, ..
        } => match window {
            None => sessions
                .create(&session)
                .map(|_| Value::Empty {})
                .map_err(io::Error::other),
            Some(_) => attached_session(sessions, attached).and_then(|s| {
                s.lock()
                    .map_err(|_| io::Error::other("session busy"))?
                    .add_window()
                    .map(|_| Value::Empty {})
            }),
        },
        Request::Attach { session, .. } => {
            sessions.get(&session)?;
            *attached = Some(session);
            Ok(Value::Empty {})
        }
        Request::Rename {
            session, name, ..
        } => {
            let s = sessions.get(&session)?;
            sessions.rename(&s, &name)?;
            if attached.as_deref() == Some(session.as_str()) {
                *attached = Some(name);
            }
            Ok(Value::Empty {})
        }
        Request::Destroy { session, .. } => {
            let s = sessions.get(&session)?;
            s.lock()
                .map_err(|_| io::Error::other("session busy"))?
                .terminate();
            sessions.destroy(&s)?;
            if attached.as_deref() == Some(session.as_str()) {
                *attached = None;
            }
            Ok(Value::Empty {})
        }
        Request::Read { session, pane, .. } => match (session, pane) {
            (Some(name), None) => {
                let session = sessions.get(&name)?;
                let s = session.lock().map_err(|_| io::Error::other("session busy"))?;
                Ok(Value::View(s.view()))
            }
            (None, Some(pane)) => attached_session(sessions, attached).and_then(|s| {
                let session = s.lock().map_err(|_| io::Error::other("session busy"))?;
                Ok(Value::Grid(session.read_pane(&pane)))
            }),
            _ => Err(io::Error::other("read takes a session or a pane")),
        },
        Request::Split { window, .. } => attached_session(sessions, attached).and_then(|s| {
            s.lock()
                .map_err(|_| io::Error::other("session busy"))?
                .split(&window)
                .map(|_| Value::Empty {})
        }),
        Request::Resize { cols, rows, .. } => attached_session(sessions, attached).and_then(|s| {
            s.lock()
                .map_err(|_| io::Error::other("session busy"))?
                .resize(cols, rows)
                .map(|_| Value::Empty {})
        }),
        Request::Spawn { pane, program, .. } => {
            attached_session(sessions, attached).and_then(|s| {
                s.lock()
                    .map_err(|_| io::Error::other("session busy"))?
                    .spawn(&pane, &program)
                    .map(|_| Value::Empty {})
            })
        }
        Request::Write { data, .. } => attached_session(sessions, attached).and_then(|s| {
            s.lock()
                .map_err(|_| io::Error::other("session busy"))?
                .write(&data)
                .map(|_| Value::Empty {})
        }),
    }
}

fn attached_session(
    sessions: &Sessions,
    attached: &Option<String>,
) -> Result<Arc<Mutex<Session>>, io::Error> {
    let name = attached
        .as_deref()
        .ok_or_else(|| io::Error::other("the client is not attached to a session"))?;
    sessions.get(name)
}

/// The socket: is the daemon running?
pub fn running(sock: &Path) -> bool {
    UnixStream::connect(sock).is_ok()
}

/// The client starts the daemon when it is not running — the same
/// binary, detached. Its output goes to a log under the state root.
pub fn ensure_running(sock: &Path, root: &Path) -> io::Result<()> {
    if running(sock) {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    if let Some(parent) = sock.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(root)?;
    let log_path = root.join("daemon.log");
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let err = log.try_clone()?;
    let mut cmd = Command::new(&exe);
    cmd.arg("daemon")
        .arg("--sock")
        .arg(sock)
        .arg("--root")
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .env("ANVIL_SOCK", sock);
    let child = cmd.spawn()?;
    // Detached: the daemon stays up when the client goes.
    drop(child);
    Ok(())
}
