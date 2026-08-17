# Daemon

The daemon owns sessions and serves clients over a unix socket.

A long-running process that owns resources and offers a service via IPC.
All state lives in the daemon: the sessions, their windows and panes,
the PTYs, the character grids. The client views a session and sends
keys.

```
daemon
 │
 └─ sessions   — named group of windows, persist on disk
```

## What it must do

Without these, it is not a daemon for this multiplexer, and no new
kernel words are needed.

1. **Sessions persist after client detach.** Close the client or drop SSH:
   sessions, windows, and panes stay. No `SIGHUP`. Only processes keep
   running. Processes outlive the client, and the daemon keeps them
   alive: when the daemon stops, the PTYs close and the processes end.
   The sessions stay on disk and reopen.
2. **PTY in the middle.** The daemon holds the master PTY; the process
   runs on the slave. On reattach, the client repaints from the daemon's
   character grid.
3. **JSON protocol over unix socket.** Clients send one JSON object
   followed by `\n`. The daemon replies the same way. No custom wire,
   no HTTP, no pairing tokens. SSH is the inter-machine bus; local
   attach is a unix socket first.
4. **Detach never kills.** Detaching the client (`prefix+q`) does not
   send SIGHUP to processes. The daemon stays up. Only the viewer
   disappears.

## Operations

The daemon answers these requests from a client over the socket.

**Sessions**

- Enumerate sessions — name every session it owns
- Create a session — give it a name
- Attach — put a client on a session
- Rename a session
- Destroy a session — its windows and panes go away; its processes
  receive `SIGHUP` when their PTY closes
- Read a session — its windows, panes, their geometry, and the
  focused pane

**Windows and panes**

- Create a window in a session
- Split a window — panes tiled to fill it
- Resize the panes — tell the processes (`SIGWINCH`)
- Spawn a process in a pane — the daemon holds the master PTY; the
  process runs on the slave
- Write to the focused pane's process — the client's keys
- Read a pane's grid — the client repaints from it

A client may drop at any time; the sessions, windows, and panes stay.

## SSH

The daemon serves a unix socket on its own machine. From another
machine, SSH carries the client: `ssh host anvil` runs the client on
that machine, and the client connects to that machine's socket. The
daemon sees only clients on the local socket. When the SSH connection
drops, the client drops with it — sessions, windows, and panes stay.

## Where things live

**Config.** One file for the whole binary: `~/.config/anvil/config.yaml`
(`XDG_CONFIG_HOME` honored). The operator's preferences.

**State.** Sessions live on disk under `~/.anvil/`, one directory per
session. The daemon owns them: the windows, panes, and character grids
persist there. The socket is `$XDG_RUNTIME_DIR/anvil.sock`
(`ANVIL_SOCK` overrides). All state lives in the daemon.

## Isolation

The daemon is a command of `anvil`. The operator can start it with
`anvil daemon`; the client starts it when it is not running. This
design exists for one reason:

**Shared state.** The daemon owns sessions that are logically part of
the anvil multiplex. Separating it would require duplicating the
session/root state or passing it over IPC, which adds complexity and
race conditions.



## References

- `docs/kernel.md` — the six kernel words and their ontology
- `protocol.md` — the wire contract, op by op
- `quarantine/src/serve/mod.rs` — the Rust source for the daemon binary
- `quarantine/src/serve/proto.rs` — the json request/response envelope
- `quarantine/src/frame/mod.rs` — session, workspace, and layout state