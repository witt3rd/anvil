# Protocol

The wire contract between the client and the daemon: one JSON object
per line over the unix socket. Requests go client to daemon; replies
come back the same way.

## Envelope

A request is one object:

```
{"id": "…", "op": "…", "args": {…}}
```

A reply is one object:

```
{"id": "…", "ok": true, "value": {…}}
{"id": "…", "ok": false, "error": "…"}
```

`id` is the client's correlation id; the daemon echoes it. `error` is
an ordinary English sentence.

## Ops

Every op is one of the documented verbs. Two verbs carry two shapes;
the args tell them apart.

**Sessions**

| op | args | reply |
|---|---|---|
| `enumerate` | — | the names of the sessions the daemon owns |
| `create` | `session` — a name | a new session with one window |
| `create` | `session`, `window` | a new window in the session |
| `attach` | `session` | the client now views this session |
| `rename` | `session`, `name` | the session under its new name |
| `destroy` | `session` | the session is gone; its windows and panes with it |
| `read` | `session` | the session's windows, their panes, each pane's geometry, and the focused pane |

**Windows and panes**

| op | args | reply |
|---|---|---|
| `split` | `window` | the window is now two panes, tiled |
| `focus` | `window` | the window becomes the current one; its first pane is focused |
| `focus` | `pane` | the pane becomes the focused pane; its window becomes current |
| `close` | `pane` | the pane is gone; its process ends; the layout re-tiles |
| `close` | `window` | the window is gone; its panes and their processes end |
| `resize` | `cols`, `rows` | the panes relaid out to the new tty; the processes told (`SIGWINCH`) |
| `spawn` | `pane`, `program` | a process runs on the pane's slave PTY; the daemon holds the master |
| `write` | `data` | the data goes to the focused pane's process |
| `read` | `pane` | the pane's grid: its cols, rows, and cells |

## Identifiers

Sessions are named. Windows and panes carry the identifier the daemon
issued when it made them; `read` returns them; the other ops address
them by it.

## Detach

The client detaches by closing the connection. The daemon forgets the
client; the sessions, windows, and panes stay.

## The guard

An op that is not in this table needs a new kernel word first. Stop
before writing it.

## References

- `docs/kernel.md` — the six kernel words and their ontology
- `daemon.md` — the daemon owns sessions and serves clients
- `client.md` — the client views a session and sends keys
- `quarantine/src/serve/proto.rs` — the reference implementation