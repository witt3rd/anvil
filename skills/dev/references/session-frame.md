---
name: session-frame
description: Design for smith's muxed session frame (roster, attach, remote, reboot).
---

# Session frame

smith is one OS window. Inside it: every **session** you own on this
anvil, switchable without changing Hyprland workspaces. Herdr's left
list is the UX scar. Herdr is not the host.

This is not in-process tiles (two panes of one session). That is a
later bruise. This is **many sessions, one frame**.

## Words

Do not import tmux's word **session** for the viewer. That collision
is how two attaches get glued to one cursor.

| We say | What it is | tmux cousin |
|---|---|---|
| **session** | Named restartable *work*: store, transcript, draft, provider, cwd. One hammer when hot. The atom in the left list. | a **window** (or a whole-window of panes). Not `tmux new-session`. |
| **frame** | One smith process. A viewer. Has its own focus (which session is in the seat) and its own view filter. | a **client** attached to a **grouped session** (`new-session -t`). |
| **pool** | All sessions owned by one `anvil serve` on one host. | the shared window group. |
| **roster** | The left list: the pool, as this frame currently filters it. | the window list of that grouped session. |
| **view** | A predicate over the pool (hot, this cwd, provider). Same sessions, different filter. Not a second pool. | not `link-window`. See below. |
| **seat** | This frame is looking at session X. Switching the roster changes *this frame's* seat only. | grouped-session current window. |
| **hot / cold** | Hammer alive vs disk only. | a window whose process is running vs a placeholder — we go cold on purpose. |

**tmux `attach -t main` (same session, two clients):** both clients share
the current window. Client A switches, B is dragged. We **do not** do
that with frames. Two smiths on one pool are *grouped viewers*: they
share sessions and hammers; each has its own seat.

**When two frames take the same seat:** they see the same live
transcript and the same hammer (identical work, real time) — like two
tmux clients looking at the *same window*. Compose is still one writer
(open: steal / refuse / fork).

**tmux `link-window`:** a window from pool A appears in pool B without
merging the pools. Our **view** is only a filter, not a portal. A
curated “also show `work:fox` here” is a later bruise. Do not build
link-window until a day needs a portal.

**tmux pane limit:** a pane cannot be transcluded alone; it belongs to
one window. Our transcludable atom is the **session** (the whole named
work). Cards inside a transcript are not linkable. In-process tiles
(smith | pty) stay inside one session; they are not roster items.

A session is not an OS process. One **anvil serve** owns the pool.
Each hot session has one hammer. A frame is a client.

## Layout (smith)

```
┌─ roster ────┬─ session (the seat) ──────────────────┐
│ views       │  you / thinking / strike / answer     │
│  all        │                                       │
│  running    │                                       │
│  this cwd   │                                       │
│─────────────│                                       │
│ fox    *    │                                       │
│ hatchling   │                                       │
│ prince/review│                                      │
│ + new       │───────────────────────────────────────│
│             │  ask                                  │
└─────────────┴───────────────────────────────────────┘
```

Switching the roster only changes which session the right side shows.
Compose draft, scroll, fold state stay with the session.

## Persist (reboot)

`~/.anvil/sessions/<id>/`

- `meta.json` — name, cwd, provider, model, created, last attached
- `namespace.pkl` — hammer store (already exists as one-store today)
- `transcript.jsonl` — cards (you / thinking / strike / answer)
- `draft` — unsent compose buffer

On boot, **nothing is hot**. `anvil serve` loads meta only. Attach
starts the hammer and replays nothing into the model context except
what we later decide to (first: show transcript in the UI; do not dump
it into the next ask). Demand-page “resume the model's memory.”

Idempotent name. Operator names it, or we mint a short word (jcode
animals are a scar we may steal). Id is a ulid/uuid; name is unique
per host.

## Attach / detach / many frames

- `anvil serve` listens on `$XDG_RUNTIME_DIR/anvil.sock` (or
  `~/.anvil/anvil.sock` if no runtime dir).
- `smith` attaches. If no server, it starts one (same as herdr).
- Detach (close smith, or a key) leaves serve + hot hammers.
- Two smiths on the same socket are **grouped frames**: same pool,
  independent seats. Switching fox→hatchling in one smith does not
  move the other.
- If both seats are fox: shared live transcript and hammer. Compose
  has one writer; others read-only until they take the seat (open:
  steal / refuse / fork).

## Remote

`smith --remote prince` = SSH stdio bridge to `anvil serve` on prince
(herdr `attach.rs` recipe). Sessions live where the hammer lives.
The frame on roger is a window onto prince's roster.

First remote: **one host per frame**. A roster that mixes roger and
prince is a later bruise (view `host=prince`).

No HTTP, no tokens, no Tailscale-specific gateway. `ssh prince smith`
must also work: remote smith, local terminal — even dumber, also
correct.

## Views

Same session set, different filters:

- all
- hot
- cwd = this directory (and children)
- provider = nim | grok | …
- name match

Views are data in `~/.anvil/views.yaml` or just hardcoded until a
bruise wants named custom views.

## Herdr / Zellij

- **Do not** spawn one herdr pane per session. That is OS workspaces
  with extra steps.
- **Do** look like herdr's left list.
- Zellij: still the PTY-tile textbook, not this feature.

## Not in this feature

- Split tiles inside one session (smith | pty).
- Streaming tokens.
- Dumping the full transcript back into the model on attach.
- Cross-host single roster.

## Build order (each is landable)

1. **Session on disk** — name, meta, per-session store; `anvil
   sessions` lists them; `smith --session fox` opens one (still
   in-process anvil). Today's default store becomes session
   `default`.
2. **Serve + attach** — daemon owns hammers; smith is a client; one
   session at a time; detach does not kill work.
3. **Roster** — left list, switch, new, rename. One frame.
4. **Multi-attach** — second smith; compose seat lock.
5. **Reboot** — serve starts on login (systemd user unit); sessions
   cold until attach.
6. **Remote** — `smith --remote host` SSH bridge.

1 is enough to stop losing work when you quit smith. 2 is the
detachable anvil. 3 is the muxed frame you asked to see. 4–6 are
the rest of the sentence.

## Open (operator decides)

- Compose seat: steal vs refuse vs fork a new session.
- Names: always typed vs mint a word if omitted.
- After reboot, auto-hot the last-attached session or stay all cold?
