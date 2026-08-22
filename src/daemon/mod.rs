//! The daemon: stays up, owns sessions, serves clients over a unix
//! socket. One JSON object per line in, one per line out. EOF is a
//! detach — the sessions, windows, and panes stay.

pub mod acp;
pub mod adopt;
pub mod grok;
pub mod keys;
pub mod pane;
pub mod sat;
pub mod session;
pub mod tiling;
pub mod watch;

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
    let sessions = Arc::new(Sessions::open(root.clone())?);
    {
        let sessions = sessions.clone();
        let root = root.clone();
        thread::spawn(move || sat::run(root, sessions));
    }
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

/// Remove a socket file that is not a live listener. A live listener
/// on the path — ours or a stranger's — stays; the stranger may be
/// the real program still in service.
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

/// The wire probe: does the listener speak the anvil protocol? A
/// connectable socket alone is not our daemon — a stranger on the
/// path must not count as running.
fn speaks_anvil(sock: &Path) -> bool {
    let Ok(mut stream) = UnixStream::connect(sock) else {
        return false;
    };
    if stream.set_read_timeout(Some(std::time::Duration::from_secs(1))).is_err() {
        return false;
    }
    let mut line = match serde_json::to_string(&Request::Enumerate { id: "probe".into() }) {
        Ok(line) => line,
        Err(_) => return false,
    };
    line.push('\n');
    if stream.write_all(line.as_bytes()).is_err() {
        return false;
    }
    let mut reply = String::new();
    let mut reader = BufReader::new(stream);
    if reader.read_line(&mut reply).is_err() || serde_json::from_str::<Reply>(&reply).is_err() {
        return false;
    }
    true
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
            Some(name) => attached_session(sessions, attached).and_then(|s| {
                s.lock()
                    .map_err(|_| io::Error::other("session busy"))?
                    .add_window(&name)
                    .map(|_| Value::Empty {})
            }),
        },
        Request::Attach { session, .. } => {
            sessions.get(&session)?;
            *attached = Some(session);
            Ok(Value::Empty {})
        }
        Request::Rename {
            session,
            name,
            window,
            note,
            ..
        } => match window {
            None => {
                let s = sessions.get(&session)?;
                sessions.rename(&s, &name)?;
                if attached.as_deref() == Some(session.as_str()) {
                    *attached = Some(name);
                }
                Ok(Value::Empty {})
            }
            Some(window) => attached_session(sessions, attached).and_then(|s| {
                let mut s = s.lock().map_err(|_| io::Error::other("session busy"))?;
                if let Some(ref note) = note {
                    s.set_note(&window, note)?;
                }
                if note.is_none() || name != window {
                    s.rename_window(&window, &name)?;
                }
                Ok(Value::Empty {})
            }),
        },
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
                let mut s = session.lock().map_err(|_| io::Error::other("session busy"))?;
                Ok(Value::View(s.view()))
            }
            (None, Some(pane)) => attached_session(sessions, attached).and_then(|s| {
                let session = s.lock().map_err(|_| io::Error::other("session busy"))?;
                Ok(Value::Grid(session.read_pane(&pane)))
            }),
            _ => Err(io::Error::other("read takes a session or a pane")),
        },
        Request::Split { window, rows, .. } => attached_session(sessions, attached).and_then(|s| {
            s.lock()
                .map_err(|_| io::Error::other("session busy"))?
                .split(&window, rows)
                .map(|_| Value::Empty {})
        }),
        Request::Focus { window, pane, .. } => attached_session(sessions, attached).and_then(|s| {
            let mut session = s.lock().map_err(|_| io::Error::other("session busy"))?;
            match (window, pane) {
                (None, Some(pane)) => session.focus_pane(&pane).map(|_| Value::Empty {}),
                (Some(window), None) => session.focus(&window).map(|_| Value::Empty {}),
                _ => Err(io::Error::other("focus takes a window or a pane")),
            }
        }),
        Request::Close { window, pane, .. } => attached_session(sessions, attached).and_then(|s| {
            let mut session = s.lock().map_err(|_| io::Error::other("session busy"))?;
            match (window, pane) {
                (None, Some(pane)) => session.close_pane(&pane).map(|_| Value::Empty {}),
                (Some(window), None) => session.close_window(&window).map(|_| Value::Empty {}),
                _ => Err(io::Error::other("close takes a window or a pane")),
            }
        }),
        Request::Resize { cols, rows, .. } => attached_session(sessions, attached).and_then(|s| {
            let gap = sessions.tiling().gap;
            s.lock()
                .map_err(|_| io::Error::other("session busy"))?
                .resize(cols, rows, gap)
                .map(|_| Value::Empty {})
        }),
        Request::Spawn {
            pane,
            program,
            acp,
            watch,
            name,
            ..
        } => attached_session(sessions, attached).and_then(|s| {
            s.lock()
                .map_err(|_| io::Error::other("session busy"))?
                .spawn(&pane, &program, acp, watch.as_deref(), name.as_deref())
                .map(|_| Value::Empty {})
        }),
        Request::Write {
            data, pane, prompt, ..
        } => attached_session(sessions, attached).and_then(|s| {
            s.lock()
                .map_err(|_| io::Error::other("session busy"))?
                .write(&data, pane.as_deref(), prompt)
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

/// The socket: is our daemon running?
pub fn running(sock: &Path) -> bool {
    speaks_anvil(sock)
}

fn pid_of(sock: &Path) -> Option<i32> {
    fs::read_to_string(pid_path(sock))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .filter(|&pid| pid > 1)
}

/// Stop the daemon at `sock`. Sessions stay on disk. Processes the
/// daemon held end. The next `ensure_running` starts this binary.
pub fn stop(sock: &Path) -> io::Result<()> {
    if let Some(pid) = pid_of(sock) {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while running(sock) || UnixStream::connect(sock).is_ok() {
        if std::time::Instant::now() > deadline {
            if let Some(pid) = pid_of(sock) {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = fs::remove_file(sock);
    let _ = fs::remove_file(pid_path(sock));
    Ok(())
}

/// Stop the daemon, then start this binary and wait until it speaks.
pub fn restart(sock: &Path, root: &Path) -> io::Result<()> {
    stop(sock)?;
    ensure_running(sock, root)
}

/// The client starts the daemon when it is not running — the same
/// binary, detached. Its output goes to a log under the state root.
/// Returns only when the wire speaks the anvil protocol, so a caller
/// that connects now talks to our daemon. A live listener that speaks
/// another protocol stays: it may be the real program in service, and
/// the caller runs against a separate socket instead.
pub fn ensure_running(sock: &Path, root: &Path) -> io::Result<()> {
    if running(sock) {
        return Ok(());
    }
    if UnixStream::connect(sock).is_ok() {
        return Err(io::Error::other(format!(
            "the socket at {} speaks another protocol; set ANVIL_SOCK for a separate daemon",
            sock.display()
        )));
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !speaks_anvil(sock) {
        if std::time::Instant::now() > deadline {
            return Err(io::Error::other(format!(
                "the daemon at {} did not come up",
                sock.display()
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}
