# Daemon

The daemon owns sessions and serves clients over a unix socket. It
is the parent of every process.

All state lives in the daemon: the sessions, their windows and
panes, each process, each pane's view. The client views a session
and sends keys.

```
daemon
 │
 └─ sessions   — named group of windows, persist on disk
```

## What it must do

1. **Sessions persist after client detach.** Close the client or
   drop SSH: sessions, windows, and panes stay. Processes keep
   running. The process stays on the turn. When the daemon stops,
   the processes end. The sessions stay on disk and reopen.
   Named agent panes spawn again. Each pane also stores the
   process's own session id and resumes *that* conversation —
   ACP `session/load` or `session/resume`, or the catalog `resume`
   argv with `{session}`. A global continue is not that: it is the
   last conversation on the box, not this pane's.
2. **The daemon is the parent.** It holds the process's input and
   output. On a PTY, that is the master; the process runs on the
   slave. On reattach, the client repaints from the daemon's view.
3. **JSON protocol over unix socket.** Clients send one JSON object
   followed by `\n`. The daemon replies the same way. SSH is the
   inter-machine bus; local attach is a unix socket.
4. **Detach keeps the processes.** Detaching the client (`prefix+q`)
   leaves the daemon up. Only the viewer disappears.

## Operations

The daemon answers these requests from a client over the socket.

**Sessions**

- Enumerate sessions — name every session it owns
- Create a session — give it a name
- Attach — put a client on a session
- Rename a session
- Destroy a session — its windows and panes go away; its processes
  receive `SIGHUP` when their PTY closes
- Read a session — its windows, panes, their geometry, the focused
  pane, and each process

**Windows and panes**

- Create a window in a session
- Split a window — panes tiled to fill it
- Resize the panes — tell the processes (`SIGWINCH`)
- Spawn a process in a pane — the daemon holds the process
- Write to a process — the focused pane, or a pane the client names
- Read a pane's view — the client repaints from it

A client may drop at any time; the sessions, windows, and panes stay.

## SSH

The daemon serves a unix socket on its own machine. From another
machine, SSH carries the client: `ssh host anvil` runs the client on
that machine, and the client connects to that machine's socket. The
daemon sees only clients on the local socket. When the SSH
connection drops, the client drops with it — sessions, windows, and
panes stay.

## Where things live

**Config.** One file for the whole binary: `~/.config/anvil/config.yaml`
(`XDG_CONFIG_HOME` honored). The operator's preferences.

**State.** Sessions live on disk under `~/.anvil/`, one directory per
session. The daemon owns them. The socket is
`$XDG_RUNTIME_DIR/anvil.sock` (`ANVIL_SOCK` overrides).

The daemon is a command of `anvil`. The operator can start it with
`anvil daemon`; the client starts it when the socket is dead. One
daemon per box. All state lives there.

`anvil --restart` (from a tree: `cargo run -- --restart`) stops the
running daemon and starts this binary, then attaches. When the
systemd --user unit owns the socket, that is `systemctl --user
restart anvil` — killing the pid from the side races the unit's
`Restart=on-failure`. Sessions stay on disk. Processes the old
daemon held end.

## References

- `docs/kernel.md` — the six kernel words and their ontology
- `protocol.md` — the wire contract, op by op
- `tui.md` — how the client draws
- `quarantine/src/serve/mod.rs` — the Rust source for the daemon binary
- `quarantine/src/serve/proto.rs` — the json request/response envelope
- `quarantine/src/frame/mod.rs` — session, workspace, and layout state
