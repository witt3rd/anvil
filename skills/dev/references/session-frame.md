---
name: session-frame
description: Design for sessions, workspaces, jigs, and smith casings (roster, attach, remote).
---

# Session frame

You launch **smith**. That starts a **casing**. The casing projects a
**jig**. The jig pulls **workspaces** from a catalog. Each workspace
is a named bag of **sessions** (and later other members). Sessions do
not belong to a sash, a casing, or a Hyprland workspace. Herdr's left
rail is the scar. Herdr is not the host.

## Words

**Session** is the work, not the viewer. Tmux used session for the
viewer-binding; that is how two attaches get one cursor. We do not.

**Workspace** is the named logical collection. A sash is a tab — UI
only. Do not name the collection after the chrome.

### The work (independent of projection)

| Word | What it is |
|---|---|
| **session** | Highest grain of work. The work piece on the anvil: store, transcript, provider, cwd, hammer when hot. Fox, hatchling. Exists whether anyone is looking. |
| **pool** | All sessions one `anvil serve` owns on one host. |
| **hot / cold** | Hammer alive vs disk only. Attach can warm. Reboot starts cold. |
| **workspace** | A named grouping of members. Members are sessions, and later a bash/pty or other tile. Lives in the **catalog**. Not a Hyprland workspace. Not a sash. |
| **catalog** | The set of named workspaces on this serve. Rail lists it. A jig pulls from it. |

Example — workspace `fleet-os`:

- anvil session: audit of all machines
- anvil session: research on organizing a heterogeneous fleet
- bash: manual poking

You name it. It sits in the catalog. Any jig can pull it.

Viewing a session is reading what the anvil already has: status,
context, activity, streaming output. Acting is a **request** to that
session: a prompt, Ctrl+C (IRQ), rename, … Many actors, one queue.
The anvil sequences them. There is no compose lock, no “seat
conflict.” One viewport or twenty is the same: input arrives, it is
handled. Which workspace or jig currently projects the session does
not change that.

### Many-to-many

```
session  ──┬──  workspace  ──┬──  jig  ──  casing (projects)
           └──  workspace  ──┘
                    └──  jig  ──  casing
```

- One session can be a member of many workspaces.
- One workspace can be pulled into many jigs.
- Two jigs can share a workspace and still be different intents.

Jig **home system management** and jig **compute saturation** might
both pull `fleet-os`. The other workspaces differ. The audit session
is the same work piece in both.

A new jig can involve the same sessions without cloning them. The
session does not care which drawing it appears on.

### The casing (pure UI — physical window shop)

| Word | What it is | Everyday |
|---|---|---|
| **pane** | One view/interaction surface. Bound to a session, a roster, a catalog list, or later a pty. | a panel |
| **sash** | A tab: a collection of panes. UI only. Often *projects* one workspace. It is not the workspace. | a tab’s layout |
| **window** | A collection of sashes. A column of the casing. Typical: rail \| main. | a column of the app |
| **casing** | A collection of windows. What `smith` launches. One terminal instance. | the app frame |
| **jig** | A named intent: which workspaces are pulled from the catalog, and how this casing projects them. Casings **recreate** the jig. Focus is **per casing**. | the shop drawing |

Typical casing, jig `compute saturation`, workspace `fleet-os` front:

```
┌─ window: rail ──┬─ window: main ────────────────────────────────┐
│ sash (one)      │ sash: fleet-os   [sash: gpu-jobs …]           │
│ ┌─ pane ──────┐ │ ┌─ pane (session: audit) ───────────────────┐ │
│ │ catalog     │ │ │ you / thinking / strike / answer          │ │
│ │  fleet-os ● │ │ └───────────────────────────────────────────┘ │
│ │  gpu-jobs   │ │ ┌─ pane (session: research) ────────────────┐ │
│ ├─ pane ──────┤ │ │ …                                         │ │
│ │ sessions    │ │ └───────────────────────────────────────────┘ │
│ │  audit  ●   │ │ ┌─ pane (bash) ─────────────────────────────┐ │
│ │  research ● │ │ │ $                                          │ │
│ └─────────────┘ │ └───────────────────────────────────────────┘ │
└─────────────────┴───────────────────────────────────────────────┘
```

Left window: catalog of workspaces + live sessions (herdr’s rail).
Right window: one sash per pulled workspace (or a slice). Each sash’s
panes are that workspace’s members.

A pane is not always a session. Roster panes *list* work. Session
panes *are* the interaction with one work piece. A bash pane is a
member that is not an anvil session.

The sash labeled `fleet-os` is chrome. Close the sash, switch jigs,
kill the casing: `fleet-os` is still in the catalog and its sessions
are still in the pool.

## Jig vs workspace vs casing

`smith` and `smith` on another tty (or `smith --remote prince` onto
that host’s serve) each get their **own casing**. If they open the
same **jig**, they pull the same workspaces and lay out the same
windows. It is a blueprint each rebuilds — not one shared cursor.

- **Workspace mutates** (add the bash member, drop the research
  session): every jig that pulls that workspace sees the new
  membership. Persist workspaces with the serve.
- **Jig mutates** (pull `gpu-jobs`, drop `backups`): every casing on
  that jig converges. Persist jigs with the serve.
- **Focus is local** (which sash is front, which roster row is
  highlighted): A does not drag B. That is tmux `new-session -t`,
  not `attach -t` the same session.

First implementation can be one jig and a small catalog. The *model*
already has named workspaces and named jigs. Do not wait for a second
intent to write the words down. `link`-style portals (a pane in jig B
bound to a session that also appears in jig A) fall out of
many-to-many membership — they are not a special later invention.

A workspace may carry a default arrangement of its members (fleet-os
opens as three stacked panes). The jig decides *which* workspaces sit
in *which* sashes. Override-per-jig is a later bruise.

## tmux, for the record

| tmux | We say |
|---|---|
| window (the work + its panes) | **session** (the work). Tiles inside a session-pane wait. |
| `new-session` / client | **casing** |
| session group (`-t`) | many casings, one **jig** |
| `attach -t` same session (shared cursor) | **do not** |
| `link-window` | a session that is a member of more than one workspace, or pulled by more than one jig |
| pane cannot leave its window | our atom is the **session**; cards are not linkable |

## Persist (reboot)

```
~/.anvil/sessions/<id>/     meta, namespace.pkl, transcript
~/.anvil/workspaces/<name>  members (session ids, later pty specs)
~/.anvil/jigs/<name>        which workspaces, how they are projected
```

On boot, serve loads sessions (cold), the catalog, and the jigs.
Nothing hot until a casing projects a workspace and a member session
needs a hammer. Do not dump the transcript into the next ask.

## Attach / remote

- Serve on `$XDG_RUNTIME_DIR/anvil.sock`.
- `smith` starts a casing on a jig; starts serve if needed.
- Close the casing: serve, catalog, jigs, and hot hammers stay.
- Remote: `smith --remote prince` or `ssh prince smith`. Sessions stay
  on prince. One host per casing first.

## Build order

1. **Session on disk** — today’s store becomes session `default`.
2. **Serve** — casing is a client; detach does not kill work.
3. **Workspace catalog** — name a grouping; rail lists it; one sash
   projects it.
4. **Named jigs** — intents that pull workspaces; many casings, local
   focus; all inputs queue on the session.
5. **Reboot** — systemd user unit; cold until projected.
6. **Remote** — SSH bridge.
7. **Mixed members** — bash/pty as a workspace member. Textbook: Zellij.

## Open

- Session names: always typed vs mint a word if omitted.
- After reboot: all cold, or warm whatever the front jig still
  points at when the first casing attaches?
- Workspace membership exclusive or many-to-many? Model is
  many-to-many (sessions are independent). Revisit if a day in
  smith wants exclusive.
