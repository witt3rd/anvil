# Client

The client is how the operator sits in the multiplexer. It is one of
the six kernel words: the client views a session and sends keys.
It donates its tty. The daemon paints.

```
daemon ──owns──▶ sessions (many)
client ──attaches──▶ session
```

## The daemon owns many sessions

The daemon stays up and owns sessions. It is the parent of every
process. One daemon holds many sessions at once. All state lives in
the daemon.

When the daemon is not running, the client starts it — the same
binary — and attaches.

## What the client does

The client:

- Lists the sessions the daemon owns
- Attaches to a session by name
- Donates stdout so the daemon can paint
- Creates a new session
- Renames a session
- Destroys a session — the operator's explicit act
- Sends keys, the wheel, and resizes (`input`)
- Writes to a pane it names (`write`)
- Detaches; the session stays

Prefix keys are multiplexer commands. The daemon is the keyboard:
it arms prefix, draws the popup, and sends anything else to the
focused pane's process.

## Sessions are named

A session is a named group of windows. The name is how the client
finds those panes again. When the client attaches, it names the
session it wants. When it creates a new session, it gives that
session a name.

## Detach

Detach drops the client. Sessions, windows, and panes stay.
Processes keep running. On reattach, the daemon paints from the
view it kept.

## References

- `docs/kernel.md` — the six kernel words and their ontology
- `daemon.md` — the daemon owns sessions, paints the tty, and serves clients
- `protocol.md` — the wire contract the client speaks
- `tui.md` — chrome: rail, roster, tiles
- `quarantine/src/serve/mod.rs` — the Rust source for the daemon binary
- `quarantine/src/serve/proto.rs` — the json request/response envelope
- `quarantine/src/frame/mod.rs` — session, workspace, and layout state
