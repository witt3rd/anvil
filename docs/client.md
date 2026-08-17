# Client

The client is how the operator sits in the multiplexer. It is one of the
six kernel words: the client views a session and sends keys.

```
daemon ──owns──▶ sessions (many)
client ──attaches──▶ session
```

## The daemon owns many sessions

The daemon stays up and owns sessions. One daemon holds many sessions at
once. All state lives in the daemon: the sessions, their windows and
panes, the PTYs, the character grids. The client views a session and
sends keys.

When the daemon is not running, the client starts it — the same
binary — and attaches.

## What the client does

The client:

- Lists the sessions the daemon owns, so the operator can see what is there
- Attaches to a session by name
- Creates a new session when the operator wants a fresh one
- Renames a session
- Destroys a session — the operator's explicit act, never a detach
- Sends keys to the focused pane's process
- Detaches without disturbing the session

## Sessions are named

A session is a named group of windows. The name is how the client finds
those panes again. When the client attaches, it names the session it
wants. When it creates a new session, it gives that session a name.

## Detach

Detach drops the client. Sessions, windows, and panes stay. No `SIGHUP`.
Processes keep running. On reattach, the client repaints from the
daemon's character grid.

## References

- `docs/kernel.md` — the six kernel words and their ontology
- `daemon.md` — the daemon owns sessions and serves clients
- `protocol.md` — the wire contract the client speaks
- `quarantine/src/serve/mod.rs` — the Rust source for the daemon binary
- `quarantine/src/serve/proto.rs` — the json request/response envelope
- `quarantine/src/frame/mod.rs` — session, workspace, and layout state