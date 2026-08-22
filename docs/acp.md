# ACP

ACP is one Client talking to one Agent over stdio. The Client
spawns the Agent. The Client dies, the Agent dies. `session/list`
lists conversations *inside* that Agent.

A person running ten agents needs something ACP does not name: a
**host**. The host is the Client of many Agents. It stays up. It
is itself an Agent to its viewers. A prompt to another agent is
`session/prompt` through the host. The roster is `session/list` on
the host.

This file is ACP used twice: once facing the children, once facing
the viewers. No new JSON-RPC method is required to start. The host
implements the Agent surface it already knows.

The daemon is this host. Kernel words stay in `docs/kernel.md`.
Chrome (rail, roster) stays in `docs/tui.md`. This page is how
those talk ACP.

## Roles

| Role | Kernel word | What it is |
|---|---|---|
| **Child** | process | An ACP Agent the host spawned. `grok`, `opencode acp`, anything that speaks the protocol on stdio. |
| **Host** | daemon | One process. ACP Client to every child. ACP Agent to every viewer. Owns each child's stdin and stdout. |
| **Viewer** | client | An ACP Client of the host. A TUI, Zed, a script, `ssh host anvil`. |

A shell is a child whose transport is a PTY. The host holds that
process the same way. This file specifies the ACP door.

```
viewer  ──ACP──▶  host  ──ACP stdio──▶  child (plugin)
                     ├──ACP stdio──▶  child (ui)
                     └──PTY────────▶  child (shell)
```

The host is the missing parent. Exclusive attach first: one viewer
holds the live ACP session on the host at a time. A later viewer
calls `session/resume`. Fan-out (two viewers on one child turn) is
a later increment; grok's leader already does that work inside one
child.

## Transport

Viewer to host: a unix socket on the box, newline-delimited
JSON-RPC, same framing as ACP
[stdio](https://agentclientprotocol.com/protocol/v1/transports).
SSH is how a remote viewer reaches the box.

Host to child: stdio, as ACP already specifies. The host is the
process that launched the child. Closing a viewer does not close
that stdin.

## Addressing

The host assigns each child a stable `sessionId` for as long as
that process lives. The id is what `session/list` returns and what
`session/prompt` names.

A child may have its own inner ACP sessions. The host's
`sessionId` names the *process*. The inner id is stored on the
pane. When the host respawns that child it calls `session/load`
(or `session/resume`) with that id so the conversation is this
pane's, not a new one.

Human names (`plugin`, `ui`, `anvil`) are `SessionInfo.title`. The
viewer shows titles. The wire uses `sessionId`.

## Roster

The host is an Agent that advertises `session/list` and
`session_info_update`.

`session/list` returns one `SessionInfo` per live child:

| Field | Meaning on the host |
|---|---|
| `sessionId` | The child's id on the host |
| `cwd` | The working directory the child was spawned with |
| `title` | The window name (`plugin`, `ui`) |
| `updatedAt` | Last `session/update` or spawn time |

State is metadata the host derives from the child's stream. Until
the schema grows a field, put it on `SessionInfo` as:

```json
{
  "sessionId": "win_plugin",
  "cwd": "/home/dt/src/li/being-plugin",
  "title": "plugin",
  "meta": {
    "state": "needs_you",
    "program": "opencode"
  }
}
```

`state` is one of:

| state | How the host knows |
|---|---|
| `idle` | No in-flight `session/prompt` to the child |
| `turning` | A `session/prompt` to the child has not returned |
| `needs_you` | The child has an unanswered `session/request_permission` or elicitation |
| `dead` | The process has exited; the host has not yet dropped the row |

The host pushes `session_info_update` when `state` or `title`
changes. Viewers do not poll. That stream is what keeps the rail
alive.

A viewer that only cares about the mux can stop here. Ten children,
one list, lights on.

## Turn

A viewer's `session/prompt` to the host names a `sessionId`. The
host forwards that prompt to the named child as `session/prompt`.
The host forwards that child's `session/update` notifications to
the viewer, with the same `sessionId`. When the child returns
`stopReason`, the host returns it.

```
viewer                 host                  child
   |-- session/prompt -->|                      |
   |                     |-- session/prompt -->|
   |                     |<-- session/update --|
   |<-- session/update --|                      |
   |                     |<-- stopReason ------|
   |<-- stopReason ------|                      |
```

`session/cancel` on the host cancels the child's in-flight prompt.

The host forwards. A PTY child renders itself. An ACP child is
painted by the client: a prompt/response loop in the pane
(transcript above, composer on the last row). That viewer is for
any ACP process, not one program.

## Permission

When a child sends `session/request_permission`, the host marks
that row `needs_you` and forwards the request to the attached
viewer. The viewer's response goes to the child.

If no viewer is attached, the request waits. The child stays
alive. That wait is the lifecycle ACP does not have. A later
`session/resume` delivers the pending request.

The host is the parent. Permission is the child's request, answered
by whoever is attached.

## Peer send

A note from one child to another is a `session/prompt` the host
sends to the destination.

**From a viewer.** The operator picks a row and types. That is
`session/prompt` on the host with that `sessionId`. Same as a
turn. On the mux wire this is `write` naming a pane.

**From a child.** The host, at `session/new` (or spawn), offers
itself as an MCP server to the child. Two tools:

- `peers` — returns the host's `session/list` (id, title, cwd, state).
- `send` — arguments `{ "sessionId": "…", "prompt": [ContentBlock…] }`.
  The host issues `session/prompt` to that sibling.

The sending child is an Agent talking to MCP. The host is an ACP
Client talking to the destination. The host is the only Client
those children have.

A further ACP method (`session/prompt` originating from a child)
earns its place after these two tools are in use.

## Attach

`initialize` / `authenticate` against the host, then:

- `session/list` — the roster
- `session/resume` `{ sessionId }` — exclusive attach to that child
- `session/new` — spawn a child (program, cwd, title). The host
  returns the new `sessionId`.
- `session/close` — detach the viewer; the child keeps running
- `session/delete` — the operator's kill; the host closes the
  child's stdin and reaps the process

`session/close` on the host is detach. `session/delete` is destroy.
Those two words stay distinct. ACP already has both.

First increment: one viewer on the host at a time. The host
refuses a second `initialize` until the first connection closes, or
it accepts the connection and `session/resume` fails while another
viewer holds that id. Either is exclusive attach.

## Spawn

The host needs a spawn record. ACP `session/new` already carries
`cwd` and `mcpServers`. The host adds, as `_meta` or a documented
extension on `session/new` params:

```json
{
  "cwd": "/home/dt/src/li/being-plugin",
  "mcpServers": [],
  "meta": {
    "program": ["opencode", "acp"],
    "title": "plugin"
  }
}
```

The host injects its own MCP server into `mcpServers` before
forwarding `session/new` to the child (for children that are ACP
from the first byte). For a child that is a TUI on a PTY, there is
no `session/new` to forward; the host only holds the PTY and the
roster row.

A child that already has a leader (grok) is still one process the
host spawned. The host is that process's parent. Inner leader
sockets stay the child's business.

## The host as parent

The host forwards prompts, updates, and permission. Their TUI stays
the place a person types when that child is focused on a PTY. The
ACP door is how a viewer or a sibling reaches the same process.

## Mapping

| Multiplexer | This file |
|---|---|
| daemon | host |
| session (named group of windows) | a filter on `session/list` (`cwd`, title prefix) |
| window | one child, one `sessionId` |
| pane | the view of that process (grid or forwarded updates) |
| process | the child |
| client | the viewer |

`write` to a named pane is `session/prompt` on the host.
The rail is `session/list`.
Detach is `session/close`.

Split, resize, and a PTY grid remain multiplexer ops. They are
how a person looks at a shell, and how a person looks at a child's
own TUI.

## Order

1. Host spawns children and holds stdio. Viewers attach over the
   unix socket. `session/list` + `session_info_update`. Exclusive
   `session/resume`. Forward `session/prompt` / `session/update` /
   `session/request_permission`.
2. `session/close` leaves the child up. Pending permission waits.
3. MCP `peers` and `send` on each child.
4. More than one viewer. Request-id namespacing. Permission goes
   to one attached viewer.

(1) turns the lights on. (3) retires the human courier.

## References

- [Architecture](https://agentclientprotocol.com/get-started/architecture)
- [Session list](https://agentclientprotocol.com/protocol/v1/session-list)
- [Session setup](https://agentclientprotocol.com/protocol/v1/session-setup)
- [Prompt turn](https://agentclientprotocol.com/protocol/v1/prompt-turn)
- [Transports](https://agentclientprotocol.com/protocol/v1/transports)
- Upstream tree: `~/src/ext/agent-client-protocol`
