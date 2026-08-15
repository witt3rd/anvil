---
name: session-frame
description: Design for sessions, workspaces, jigs (logical) and casings, sashes, panes (UI).
---

# Session frame

Two vocabularies. Keep them apart. They map, mostly 1:1, not entirely.

You launch **smith**. That starts a **casing** (UI). The casing
projects a **jig** (logical). The jig pulls **workspaces** from a
**catalog**. Each workspace is a named bag of **sessions** (and later
other members). Sessions do not belong to a sash, a casing, or a
Hyprland workspace. Herdr's left rail is the scar. Herdr is not the
host.

## Logical

These exist with no smith open. Persist them. Act on them through
any projection.

| Word | What it is |
|---|---|
| **session** | Highest grain of work. The work piece on the anvil: store, transcript, provider, cwd, hammer when hot. Fox, hatchling. Exists whether anyone is looking. |
| **pool** | All sessions one `anvil serve` owns on one host. |
| **hot / cold** | Hammer alive vs disk only. Attach can warm. Reboot starts cold. |
| **member** | One occupant of a workspace: a session, later a bash/pty or other tile. |
| **workspace** | A named grouping of members. Lives in the catalog. Not a Hyprland workspace. Not a sash. |
| **catalog** | The set of named workspaces on this serve. A jig pulls from it. |
| **jig** | A named intent: *which* workspaces. `home system management` vs `compute saturation`. Not a casing. Not a layout. |

Example — workspace `fleet-os`:

- anvil session: audit of all machines
- anvil session: research on organizing a heterogeneous fleet
- bash: manual poking

You name it. It sits in the catalog. Any jig can pull it.

Viewing a session is reading what the anvil already has: status,
context, activity, streaming output. Acting is a **request** to that
session: a prompt, Ctrl+C (IRQ), rename, … Many actors, one queue.
The anvil sequences them. There is no compose lock, no “seat
conflict.” Which workspace or jig currently includes the session
does not change that.

Many-to-many, all on this side:

```
session  ──┬──  workspace  ──┬──  jig
           └──  workspace  ──┘
                    └──  jig
```

- One session can be a member of many workspaces.
- One workspace can be pulled into many jigs.
- Two jigs can share a workspace and still be different intents.

Jig **home system management** and jig **compute saturation** might
both pull `fleet-os`. The other workspaces differ. The audit session
is the same work piece in both. A new jig can involve the same
sessions without cloning them.

## UI

These exist only while a smith is sitting there. They *project*
logical things. Closing them does not delete the logical thing.

| Word | What it is | Everyday |
|---|---|---|
| **pane** | One view/interaction surface. | a panel |
| **sash** | A tab: a collection of panes. | a tab’s layout |
| **window** | A collection of sashes. A column of the casing. Typical: rail \| main. | a column of the app |
| **casing** | A collection of windows. What `smith` launches. One terminal instance. | the app frame |
| **focus** | Which sash is front, which pane, which roster row. **Per casing.** | the local cursor |

Typical casing, projecting jig `compute saturation`, workspace
`fleet-os` front:

```
┌─ window: rail ──┬─ window: main ────────────────────────────────┐
│ sash (one)      │ sash: fleet-os   [sash: gpu-jobs …]           │
│ ┌─ pane ──────┐ │ ┌─ pane (session: audit) ───────────────────┐ │
│ │ catalog     │ │ │ you / thinking / strike / answer          │ │
│ │  fleet-os ● │ │ └───────────────────────────────────────────┘ │
│ │  gpu-jobs   │ │ ┌─ pane (session: research) ────────────────┐ │
│ ├─ pane ──────┤ │ │ …                                         │ │
│ │ pool        │ │ └───────────────────────────────────────────┘ │
│ │  audit  ●   │ │ ┌─ pane (bash) ─────────────────────────────┐ │
│ │  research ● │ │ │ $                                          │ │
│ └─────────────┘ │ └───────────────────────────────────────────┘ │
└─────────────────┴───────────────────────────────────────────────┘
```

`smith` and `smith` on another tty (or `smith --remote prince`) each
get their **own casing**. Same jig → same *logical* set of workspaces.
Each casing rebuilds its own chrome. Focus does not travel. That is
tmux `new-session -t`, not `attach -t` the same session.

## Mapping (mostly 1:1, not entirely)

```
logical:   session / member     workspace      catalog / pool     jig
               ↕                    ↕               ↕              ↕
ui:            pane                sash         rail panes       casing
```

| Logical | Usually projects as | Not 1:1 because |
|---|---|---|
| **session** / **member** | **pane** | Many panes can show one session. A pane can be a roster or catalog list — no session behind it. A bash member is not an anvil session. |
| **workspace** | **sash** | Closing the sash does not delete the workspace. One workspace can appear in many casings (many sashes). A sash might show a *slice* of a workspace. |
| **catalog** | rail catalog pane | The catalog is the set. The rail is one browser. |
| **pool** | rail sessions pane | Same: the set vs one browser. |
| **jig** | **casing** | Many casings project one jig. A casing also has chrome (rail, focus) that is not the jig. |

**Window** has no logical twin. It is how a casing is split (rail |
main). Do not invent a logical noun for it.

A workspace may carry a **default projection** of its members
(fleet-os opens as three stacked panes). That is presentation hung
on a logical object, not a sash, and not part of the jig. The jig
names *which* workspaces. The casing maps them to sashes. Override
per casing is a later bruise.

Mutations stay on the side they belong to:

- **Workspace mutates** (add the bash, drop the research session):
  every jig that pulls it sees new membership. Persist with the serve.
- **Jig mutates** (pull `gpu-jobs`, drop `backups`): every casing
  projecting that jig converges on the new set. Persist with the serve.
- **Focus mutates**: local to the casing. Never persist as if it were
  the jig.

`link`-style portals fall out of many-to-many membership. They are
not a special UI invention.

## tmux, for the record

| tmux | We say |
|---|---|
| window (the work + its panes) | **session** (logical). Tiles inside a session-pane wait. |
| `new-session` / client | **casing** (UI) |
| session group (`-t`) | many casings projecting one **jig** |
| `attach -t` same session (shared cursor) | **do not** |
| `link-window` | a session in more than one workspace, or a workspace in more than one jig |
| pane cannot leave its window | our atom is the **session**; cards are not linkable |

## Persist (reboot)

Logical (serve, survive no casing):

```
~/.anvil/sessions/<id>/     meta, namespace.pkl, transcript
~/.anvil/workspaces/<name>  members
~/.anvil/jigs/<name>        which workspaces
```

UI (optional, later): a casing's last focus, split sizes. Do not
stuff those into the jig file.

On boot, serve loads sessions (cold), the catalog, and the jigs.
Nothing hot until a casing projects a workspace and a member session
needs a hammer. Do not dump the transcript into the next ask.

## Attach / remote

- Serve on `$XDG_RUNTIME_DIR/anvil.sock`.
- `smith` starts a casing projecting a jig; starts serve if needed.
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
  names when the first casing attaches?
- Workspace membership exclusive or many-to-many? Model is
  many-to-many (sessions are independent). Revisit if a day in
  smith wants exclusive.
