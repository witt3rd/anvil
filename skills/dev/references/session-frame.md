---
name: session-frame
description: Design for smith casings over a pool of sessions (roster, attach, remote).
---

# Session frame

You launch **smith**. That starts a **casing**. Inside it you see every
**session** (work piece) on this anvil, without changing Hyprland
workspaces. Herdr's left rail is the scar. Herdr is not the host.

## Words

**Session** is the work, not the viewer. Tmux used session for the
viewer-binding; that is how two attaches get one cursor. We do not.

### The work

| Word | What it is |
|---|---|
| **session** | Highest grain of work. The work piece on the anvil: store, transcript, provider, cwd, hammer when hot. Fox, hatchling. |
| **pool** | All sessions one `anvil serve` owns on one host. |
| **hot / cold** | Hammer alive vs disk only. Attach can warm. Reboot starts cold. |

Viewing a session is reading what the anvil already has: status,
context, activity, streaming output. Acting is a **request** to that
session: a prompt, Ctrl+C (IRQ), rename, … Many actors, one queue.
The anvil sequences them. There is no compose lock, no “seat
conflict.” One viewport or twenty is the same: input arrives, it is
handled.

### The casing (pure UI — physical window shop)

| Word | What it is | Everyday |
|---|---|---|
| **pane** | One view/interaction surface. May be bound to a session (transcript + ask) or be a roster / group list. | a panel |
| **sash** | A collection of panes (split). | a tab’s layout |
| **window** | A collection of sashes. | a column of the app |
| **casing** | A collection of windows. What `smith` launches. One terminal instance. | the app frame |
| **jig** | The shared blueprint: which windows/sashes/panes exist and which session (if any) each pane is bound to. Casings **recreate** the jig. Focus (which sash, which pane) is **per casing**. | the shop drawing |

Typical casing:

```
┌─ window: rail ──┬─ window: workspace ─────────────────┐
│ sash (one)      │ sash: “main”    [sash: “notes” …]   │
│ ┌─ pane ──────┐ │ ┌─ pane (session fox) ────────────┐ │
│ │ groups      │ │ │ you / thinking / strike / answer │ │
│ │ (herdr top) │ │ │                                  │ │
│ ├─ pane ──────┤ │ └──────────────────────────────────┘ │
│ │ sessions    │ │ ask                                  │
│ │ (herdr bot) │ │                                      │
│ └─────────────┘ │                                      │
└─────────────────┴──────────────────────────────────────┘
```

Left window: one sash, one or two panes (groups + live sessions).
Right window: one or more sashes (tabs); each sash has one or more
panes (a session, later a pty).

A pane is not always a session. Roster panes *list* work. Session
panes *are* the interaction with one work piece.

## Jig vs casing

`smith` and `smith` on another tty (or `smith --remote prince` onto
that host’s serve) each get their **own casing**. If they attach to
the same **jig**, they lay out the same windows. It is a blueprint
each rebuilds — not one shared cursor.

- **Jig mutates** (add sash, bind pane to hatchling): every casing on
  that jig should converge. Persist the jig with the serve.
- **Focus is local** (which sash is front, which roster row is
  highlighted): A does not drag B. That is tmux `new-session -t`,
  not `attach -t` the same session.

First cut: one jig per host (the serve’s default jig). Named jigs
(“monitoring” vs “work”) are a later bruise — that is when
`link`-style portals matter (a pane in jig B bound to a session that
also appears in jig A). Until then, one pool, one jig, many casings.

## tmux, for the record

| tmux | We say |
|---|---|
| window (the work + its panes) | **session** (the work). Tiles inside a session-pane wait. |
| `new-session` / client | **casing** |
| session group (`-t`) | many casings, one **jig** |
| `attach -t` same session (shared cursor) | **do not** |
| `link-window` | later: a pane on jig B bound to a session from the same pool |
| pane cannot leave its window | our atom is the **session**; cards are not linkable |

## Persist (reboot)

`~/.anvil/sessions/<id>/` — meta, `namespace.pkl`, transcript.
`~/.anvil/jig.json` — the blueprint (not focus).

On boot, serve loads meta + jig. Nothing hot until a casing binds a
pane to a session and that session needs a hammer. Do not dump the
transcript into the next ask.

## Attach / remote

- Serve on `$XDG_RUNTIME_DIR/anvil.sock`.
- `smith` starts a casing; starts serve if needed.
- Close the casing: serve and hot hammers stay.
- Remote: `smith --remote prince` or `ssh prince smith`. Sessions stay
  on prince. One host per casing first.

## Build order

1. **Session on disk** — today’s store becomes session `default`.
2. **Serve** — casing is a client; detach does not kill work.
3. **Jig + roster casing** — rail + workspace; one session pane.
4. **Many casings** — same jig, local focus; all inputs queue on the session.
5. **Reboot** — systemd user unit; cold until bound.
6. **Remote** — SSH bridge.

## Open

- Session names: always typed vs mint a word if omitted.
- After reboot: all cold, or warm whatever the jig still points at
  when the first casing attaches?
