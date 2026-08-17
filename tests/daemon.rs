//! The wire contract, end to end: a real `anvil daemon` process over
//! a real unix socket, speaking the ops of `docs/protocol.md`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anvil::proto::{Reply, Request, Value};

struct Daemon {
    child: Child,
    sock: PathBuf,
}

impl Daemon {
    fn start(dir: &Path) -> Daemon {
        let sock = dir.join("anvil.sock");
        let root = dir.join("root");
        let child = Command::new(env!("CARGO_BIN_EXE_anvil"))
            .arg("daemon")
            .arg("--sock")
            .arg(&sock)
            .arg("--root")
            .arg(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let daemon = Daemon { child, sock };
        daemon.wait_until_up();
        daemon
    }

    fn wait_until_up(&self) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if self.sock.exists() && UnixStream::connect(&self.sock).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("daemon did not come up");
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.sock);
        let _ = std::fs::remove_file(self.sock.with_extension("pid"));
    }
}

struct Client {
    reader: BufReader<UnixStream>,
    stream: UnixStream,
    next: u64,
}

impl Client {
    fn connect(sock: &Path) -> Client {
        let stream = UnixStream::connect(sock).unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        Client {
            reader,
            stream,
            next: 0,
        }
    }

    fn send(&mut self, op: impl FnOnce(&str) -> Request) -> Reply {
        self.next += 1;
        let id = self.next.to_string();
        let request = op(&id);
        let mut line = serde_json::to_string(&request).unwrap();
        line.push('\n');
        self.stream.write_all(line.as_bytes()).unwrap();
        self.stream.flush().unwrap();
        let mut reply = String::new();
        self.reader.read_line(&mut reply).unwrap();
        let reply: Reply = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply.id, id, "reply echoes the id: {reply:?}");
        reply
    }

    fn ok(&mut self, op: impl FnOnce(&str) -> Request) -> Value {
        let reply = self.send(op);
        assert!(reply.ok, "expected ok, got {reply:?}");
        reply.value.unwrap_or(Value::Empty {})
    }

    fn err(&mut self, op: impl FnOnce(&str) -> Request) -> String {
        let reply = self.send(op);
        assert!(!reply.ok, "expected an error, got {reply:?}");
        reply.error.expect("error sentence")
    }
}

fn wait_for(mut grid: anvil::daemon::pane::Grid, done: impl Fn(&anvil::daemon::pane::Grid) -> bool) {
    for _ in 0..100 {
        if done(&grid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_wire_flow_create_attach_split_spawn_write_rename_destroy() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(dir.path());
    let mut client = Client::connect(&daemon.sock);

    // enumerate — nothing yet
    let Value::Sessions { sessions } = client.ok(|id| Request::Enumerate { id: id.into() }) else {
        panic!("expected session names")
    };
    assert!(sessions.is_empty());

    // create a session; attaching to a missing one is an error
    assert!(client
        .err(|id| Request::Attach {
            id: id.into(),
            session: "nope".into(),
        })
        .contains("no such session"));
    client.ok(|id| Request::Create {
        id: id.into(),
        session: "work".into(),
        window: None,
    });
    let Value::Sessions { sessions } = client.ok(|id| Request::Enumerate { id: id.into() }) else {
        panic!("expected session names")
    };
    assert_eq!(sessions, vec!["work".to_string()]);

    // attach, read the session: one window, one pane, focused
    client.ok(|id| Request::Attach {
        id: id.into(),
        session: "work".into(),
    });
    let Value::View(view) = client.ok(|id| Request::Read {
        id: id.into(),
        session: Some("work".into()),
        pane: None,
    }) else {
        panic!("expected a session view")
    };
    assert_eq!(view.windows.len(), 1);
    assert_eq!(view.windows[0].panes.len(), 1);
    assert_eq!(view.focused, "1");

    // split the window: two panes, tiled to the tty
    client.ok(|id| Request::Split {
        id: id.into(),
        window: "1".into(),
    });
    let Value::View(view) = client.ok(|id| Request::Read {
        id: id.into(),
        session: Some("work".into()),
        pane: None,
    }) else {
        panic!("expected a session view")
    };
    assert_eq!(view.windows[0].panes.len(), 2);
    let a = &view.windows[0].panes[0];
    let b = &view.windows[0].panes[1];
    assert_eq!(a.cols + b.cols, 80);
    assert_eq!(a.rows, 24);
    assert_eq!(b.rows, 24);

    // resize the tty: the panes relay out
    client.ok(|id| Request::Resize {
        id: id.into(),
        cols: 100,
        rows: 40,
    });
    let Value::View(view) = client.ok(|id| Request::Read {
        id: id.into(),
        session: Some("work".into()),
        pane: None,
    }) else {
        panic!("expected a session view")
    };
    assert_eq!(view.windows[0].panes[0].cols + view.windows[0].panes[1].cols, 100);
    assert_eq!(view.windows[0].panes[0].rows, 40);

    // spawn a process in the focused pane, write to it, read its grid
    client.ok(|id| Request::Spawn {
        id: id.into(),
        pane: "1".into(),
        program: "sh".into(),
    });
    client.ok(|id| Request::Write {
        id: id.into(),
        data: "printf 'hello wire'\n".into(),
    });
    let Value::Grid(grid) = client.ok(|id| Request::Read {
        id: id.into(),
        session: None,
        pane: Some("1".into()),
    }) else {
        panic!("expected a grid")
    };
    wait_for(grid, |g| g.lines.iter().any(|l| l.contains("hello wire")));
    let Value::Grid(grid) = client.ok(|id| Request::Read {
        id: id.into(),
        session: None,
        pane: Some("1".into()),
    }) else {
        panic!("expected a grid")
    };
    assert!(grid.lines.iter().any(|l| l.contains("hello wire")), "{grid:?}");
    assert!(grid.alive);

    // rename the session; attach follows the new name
    client.ok(|id| Request::Rename {
        id: id.into(),
        session: "work".into(),
        name: "deep".into(),
    });
    let Value::Sessions { sessions } = client.ok(|id| Request::Enumerate { id: id.into() }) else {
        panic!("expected session names")
    };
    assert_eq!(sessions, vec!["deep".to_string()]);
    client.ok(|id| Request::Write {
        id: id.into(),
        data: "printf 'still attached'\n".into(),
    });
    let Value::Grid(grid) = client.ok(|id| Request::Read {
        id: id.into(),
        session: None,
        pane: Some("1".into()),
    }) else {
        panic!("expected a grid")
    };
    wait_for(grid, |g| g.lines.iter().any(|l| l.contains("still attached")));

    // destroy: the session is gone
    client.ok(|id| Request::Destroy {
        id: id.into(),
        session: "deep".into(),
    });
    let Value::Sessions { sessions } = client.ok(|id| Request::Enumerate { id: id.into() }) else {
        panic!("expected session names")
    };
    assert!(sessions.is_empty());
}

#[test]
fn detach_keeps_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(dir.path());

    let mut client = Client::connect(&daemon.sock);
    client.ok(|id| Request::Create {
        id: id.into(),
        session: "work".into(),
        window: None,
    });
    client.ok(|id| Request::Attach {
        id: id.into(),
        session: "work".into(),
    });
    client.ok(|id| Request::Spawn {
        id: id.into(),
        pane: "1".into(),
        program: "sh".into(),
    });
    client.ok(|id| Request::Write {
        id: id.into(),
        data: "printf 'long live the session'\n".into(),
    });
    drop(client); // detach: EOF

    // A new client attaches; the session and its process stay.
    let mut again = Client::connect(&daemon.sock);
    let Value::View(view) = again.ok(|id| Request::Read {
        id: id.into(),
        session: Some("work".into()),
        pane: None,
    }) else {
        panic!("expected a session view")
    };
    assert_eq!(view.focused, "1");
    again.ok(|id| Request::Attach {
        id: id.into(),
        session: "work".into(),
    });
    let Value::Grid(grid) = again.ok(|id| Request::Read {
        id: id.into(),
        session: None,
        pane: Some("1".into()),
    }) else {
        panic!("expected a grid")
    };
    wait_for(grid, |g| g.lines.iter().any(|l| l.contains("long live the session")));
    let Value::Grid(grid) = again.ok(|id| Request::Read {
        id: id.into(),
        session: None,
        pane: Some("1".into()),
    }) else {
        panic!("expected a grid")
    };
    assert!(grid.alive, "the process outlives the client");
}

#[test]
fn destroy_sighups_the_processes() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("hupped");
    let ready = dir.path().join("ready");
    let daemon = Daemon::start(dir.path());

    let mut client = Client::connect(&daemon.sock);
    client.ok(|id| Request::Create {
        id: id.into(),
        session: "work".into(),
        window: None,
    });
    client.ok(|id| Request::Attach {
        id: id.into(),
        session: "work".into(),
    });
    client.ok(|id| Request::Spawn {
        id: id.into(),
        pane: "1".into(),
        program: "sh".into(),
    });
    client.ok(|id| Request::Write {
        id: id.into(),
        data: format!(
            "trap 'echo hupped > {}' HUP; echo ready > {}; while :; do sleep 1; done\n",
            marker.display(),
            ready.display()
        )
        .into(),
    });

    let start = Instant::now();
    while !ready.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the shell never installed the trap"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    client.ok(|id| Request::Destroy {
        id: id.into(),
        session: "work".into(),
    });

    let start = Instant::now();
    while !marker.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the process never received SIGHUP"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn ops_without_an_attached_session_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(dir.path());
    let mut client = Client::connect(&daemon.sock);

    assert!(client
        .err(|id| Request::Write {
            id: id.into(),
            data: "echo x".into(),
        })
        .contains("not attached"));
    assert!(client
        .err(|id| Request::Read {
            id: id.into(),
            session: None,
            pane: Some("1".into()),
        })
        .contains("not attached"));
    assert!(client
        .err(|id| Request::Read {
            id: id.into(),
            session: Some("work".into()),
            pane: Some("1".into()),
        })
        .contains("takes a session or a pane"));
}