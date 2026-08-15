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

| Word | What it is |
|---|---|
| **session** | Named, restartable work: store + transcript + compose draft + provider/model + cwd. One hammer when hot. |
| **frame** | One smith process: roster + the session you are looking at. |
| **roster** | Left list of sessions. Filtered views over the same set. |
| **view** | A named predicate (running, cwd prefix, provider, host). Not a second store. |
| **attach** | A frame is looking at a session. Many frames may attach to one session. |
| **hot / cold** | Hot: hammer process alive. Cold: persisted on disk only. Attach makes it hot. |

A session is not an OS process. One **anvil serve** on the machine owns
all sessions. Each hot session has one hammer. smith is a client.

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
- Two smiths on the same socket: both see the roster; both can view
  the same session. Writes (ask, strike) are serialized on the
  session. Last attach's compose draft wins if we do not lock —
  **lock compose to one attacher**; others read-only until they take
  the seat. (Open: steal-seat vs fork-draft.)

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
