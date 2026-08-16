---
name: session-frame
description: Conceptual vs UX taxonomy; services vs fibers; event log; slots.
---

# Session frame

Two vocabularies. Keep them apart. They map, mostly 1:1, not entirely.

Opposite lifetime rules. That is the distinction.

You launch a **casing** (binary `smith`). It loads a **layout**. The
layout is an arrangement of a **catalog**. The catalog names
**workspaces**. Each workspace is a bag of **members**. An anvil
**session** is one kind of member. Herdr's left rail is the scar.
Herdr is not the host.

## Conceptual

Independent objects. Destroying a collection does **not** destroy
its occupants. An occupant may appear in any number of collections.

| Word | What it is |
|---|---|
| **session** | Anvil-specific: one coherent agentic process. Store, transcript, provider, cwd, hammer when hot. |
| **member** | A machine process. Usually a session. Also a bash terminal, a web client, … |
| **workspace** | A collection of members. Everyday: the **bench**. Not a Hyprland workspace. Not a sash. |
| **catalog** | A collection of workspaces. A named intent (`home system management`, `compute saturation`). |

`fleet-os` is a workspace: audit session + research session + bash.
It sits in as many catalogs as want it.

```
session  ⊂  member  ──┬──  workspace  ──┬──  catalog
                      └──  workspace  ──┘
                               └──  catalog
```

- Destroy the workspace: members remain. A member may be in many
  workspaces.
- Destroy the catalog: workspaces remain. A workspace may be in many
  catalogs.

Viewing a session is reading what the anvil already has. Acting is a
**request** on that session (prompt, IRQ, rename, …). Many actors,
one queue. No compose lock. Which workspace or catalog includes the
session does not change that.

**hot / cold** is adapter-fiber state on a member (hammer alive vs
disk only), not a UX word. The serve owns the processes. Reboot
starts cold. A member may also be **failed** (adapter died and has
not yet been hung again).

There is no **jig**. Catalog is which workspaces. Layout (UI) is how
a catalog is arranged.

## UX

A tree. Destroying a parent **does** destroy its children. A child
belongs to exactly one parent.

| Word | What it is | Everyday |
|---|---|---|
| **pane** | Exposes a member for view and interaction. | a panel |
| **sash** | A collection of panes. | a tab, a list |
| **window** | A collection of sashes. | a column of the app |
| **layout** | A saved instance and arrangement of windows, sashes, and panes **of a catalog**. | the shop drawing, on disk |
| **casing** | The live surface of a layout. What `smith` launches. | the app |
| **focus** | Which sash/pane/row is front. **Per casing.** | the local cursor |

When the member is an anvil session, that pane is a **smith**.

```
pane  ∈₁  sash  ∈₁  window  ∈₁  casing
                 layout  →  many casings (local or remote)
```

- Destroy the pane: the member remains. A member may appear in any
  number of panes.
- Destroy the sash: its panes die. A pane lives in exactly one sash.
- Destroy the window: its sashes die. A sash lives in exactly one
  window.
- Destroy the casing: its windows die. A window lives in exactly one
  casing. The layout on disk remains. Any number of casings can load
  it.

Typical casing, layout of catalog `compute saturation`, workspace
`fleet-os` front:

```
┌─ window: rail ──┬─ window: main ────────────────────────────────┐
│ sash (list)     │ sash: fleet-os   [sash: gpu-jobs …]           │
│ ┌─ pane ──────┐ │ ┌─ pane / smith (session: audit) ───────────┐ │
│ │ catalogs    │ │ │ you / thinking / strike / answer          │ │
│ │  compute ●  │ │ └───────────────────────────────────────────┘ │
│ │  home       │ │ ┌─ pane / smith (session: research) ────────┐ │
│ ├─ pane ──────┤ │ │ …                                         │ │
│ │ workspaces  │ │ └───────────────────────────────────────────┘ │
│ │  fleet-os ● │ │ ┌─ pane (member: bash) ─────────────────────┐ │
│ │  gpu-jobs   │ │ │ $                                          │ │
│ └─────────────┘ │ └───────────────────────────────────────────┘ │
└─────────────────┴───────────────────────────────────────────────┘
```

`smith` and `smith` on another tty (or `smith --remote prince`) each
get their **own casing**. Same layout file → same arrangement of the
same catalog. Focus does not travel.

## Mapping (mostly 1:1, not entirely)

```
conceptual:  member          workspace         catalog
                 ↕                ↕            ↕        ↕
ux:            pane             sash         layout    casing
```

| Conceptual | Usually maps to | Not 1:1 because |
|---|---|---|
| **member** | **pane** | Many panes can expose one member. Destroying the pane does not destroy the member. |
| **session** | **smith** (a pane) | Smith is the pane only when the member is an anvil session. Bash is a pane, not a smith. |
| **workspace** | **sash** | Sash owns its panes (tree). Workspace does not own its members (shared). Closing the sash does not destroy the workspace. |
| **catalog** | **layout** + **casing** | Two UX words: layout is the saved arrangement of that catalog; casing is one live instance. Many casings, one layout. |

**Window** has no conceptual twin. It is how a layout is split
(rail | main). Do not invent a conceptual noun for it.

A sash that is a **list** (the rail) browses catalogs and workspaces.
That is chrome of the layout, not a member.

## Direction (services, fibers, log, slots)

Scar: Cordis (spatiotemporal composability) and DeepSeek Harness
(inspect / mount / unmount; trajectory). Not a host. We keep our
words. We take the invariants.

**Spatial / temporal, across our two vocabularies.** A **member** is
a *service* (provided, persists). A **pane** (and the adapter that
warms a hammer) is a *fiber* (injects the service; dispose reverts
its effects). Workspace and catalog are loader groups, not owners.
Serve is the root context.

**The bruise stays in smith.** Demand paging moves in band: feel it
in the seat, ask *smith* to mount a fiber, promote it if it earns
its keep. Do not walk to another agent to edit Rust for a clock.

Three verbs, later:

| Verb | What it does |
|---|---|
| **inspect** | Live fibers, services, slots. Catalog ∩ running store. |
| **mount** | Evaluate a temporary member/plugin. In memory. Same trust as a strike. |
| **unmount** | Dispose to quiescence. Every listener, timer, pane, hammer effect gone. |

Temporary mounts do not survive restart. Keeping one means writing a
real member. No silent promote.

**Slots** are named seats on the casing a live fiber can occupy (a
rail chip, a sash, a status, a clock pane). Without slots there is
nowhere to hang what smith writes. Without total unmount, a mount
leaks.

**The event log is the product.** One append-only stream per member
(and serve lifecycle). Cards, the next ask, a trajectory sash, and
telemetry are *projections*. **Model-visible means logged** — if the
model saw it, the log can reconstruct it. Do not dump the transcript
into the next ask; derive. Timing (prefill, TTFT, decode, reasoning,
tok/s, strike wall, respawn) is data on that stream (`Timing` on
Ask/Strike/Answer) and on the live ring (`src/prof.rs`, `anvil inspect`
/ `anvil prof`). Zoom/diagnose is a workspace member that injects the
same session, not a debug flag.

Self-mod without a log is a trick you cannot debug. A log without
slots is a file you `less`.

Mutations stay on the side they belong to:

- **Workspace mutates** (add the bash, drop the research session):
  every catalog that includes it sees new membership.
- **Catalog mutates** (pull `gpu-jobs`, drop `backups`): every layout
  of that catalog names a new set. Layouts may need a pass.
- **Layout mutates** (split, retab): casings loading that layout
  converge. Focus stays per casing.

## tmux, for the record

| tmux | We say |
|---|---|
| window (the work + its panes) | **session** (conceptual). Tiles inside a smith wait. |
| `new-session` / client | **casing** |
| session group (`-t`) | many casings of one **layout** |
| `attach -t` same session (shared cursor) | **do not** |
| `link-window` | a member in more than one workspace, or a workspace in more than one catalog |
| pane cannot leave its window | UI tree is exclusive; the **member** is not |

## Persist (reboot)

Conceptual (serve, survive no casing):

```
~/.anvil/sessions/<id>/      session member (meta, namespace.pkl, event log)
~/.anvil/workspaces/<name>   members
~/.anvil/catalogs/<name>     which workspaces
```

UX, saved:

```
~/.anvil/layouts/<name>      arrangement of a catalog (windows, sashes, panes)
```

Focus and live split drag stay on the casing. Do not stuff them into
the catalog.

On boot, serve loads members (cold), workspaces, catalogs, layouts.
Nothing hot until a casing exposes a session that needs a hammer.
The next ask is a projection of the event log, not a paste of cards.

## Attach / remote

- Serve on `$XDG_RUNTIME_DIR/anvil.sock`.
- `smith` starts a casing on a layout; starts serve if needed.
- Close the casing: serve, members, workspaces, catalogs, layouts,
  and hot hammers stay.
- Remote: `smith --remote prince` / `anvil --remote prince inspect`
  is `ssh [-t] prince -- smith|anvil …`. Sessions stay on that host.
  That host needs anvil on PATH and (for boot) `anvil serve --install`.
  One host per casing first.

## Build order

1. **Session on disk** — done. Today’s store is session `default`
   (legacy `~/.anvil/default` still counts).
2. **Serve** — done. Unix socket; smith attaches; hammers outlive the
   casing. No systemd unit yet (reboot is later).
3. **Workspace + catalog** — done as persist + CLI + rail.
4. **Event log** — first cut. `events.jsonl` is the source. Serve
   appends. Cards project. Ask projects model-visible events only.
   Legacy `transcript.jsonl` migrates.
5. **Slots + inspect + mount** — first cut. `anvil inspect`;
   `anvil mount clock` occupies `casing.status`; `anvil unmount dyn-1`
   disposes to quiescence. Temporary, in memory.
6. **Trajectory sash** — first cut. Alt+L in smith lists the event
   log (seq, visible, kind, timing). Not a zoomed timeline yet.
7. **Layout geometry + many casings** — first cut. Sash tabs
   (workspaces); Alt+[ ] cycle; every member of the front workspace
   stacks; Alt+J / Alt+K cycle focus. Many casings already attach
   to one serve.
8. **Reboot** — first cut. `anvil serve --install` writes
   `~/.config/systemd/user/anvil.service`. First casing `warm`s
   the front workspace (hammers, PTYs, clock). Temporary mounts
   that are not members do not survive.
9. **Remote** — first cut. `smith --remote HOST` / `anvil --remote HOST`
   is SSH; sessions stay on HOST. Linger on `--install`.
10. **Mixed members** — first cut. Bash is a `MemberRef::Pty`. Serve
    owns `$SHELL` (portable-pty + vt100). smith projects the screen.
    `anvil workspace add <ws> bash --pty`. Rail `p` names one. Keys
    go to the PTY when that pane is focused; Ctrl+Q closes the casing.
    `anvil pty snap` / `anvil pty write` / inspect `casing.main` text
    is the live screen. `MemberRef::Edit` is a scratch buffer at
    `edits/<id>.txt` (autosave). Every workspace member stacks.
    Web / promoted mounts still later. Textbook: Zellij. Not a host.

## Open

- Web member. Promoted mounts beyond clock.
- Zoomed timeline (log pane is the first diagnose member).
- Drag-resize (Alt+= / Alt+- persist weights). Mouse click/wheel
  already hit-tests panes, tabs, rail rows, and sash pills.
- Theme is UX, not conceptual. Every painted surface is a dotted
  face in `src/tui/theme.rs` (`message.user.field`, `hint.key`, …).
  Add the face before the widget. Packs fill inks; `theme:` in
  config overrides.
- Prof is first-class. New serve op / fiber / model phase gets a
  `prof::span` (and a `Timing` field if it is logged). `ANVIL_TRACE=1`
  for the tracing subscriber.
- Keys: every action is named in `src/tui/keys.rs`. Defaults follow
  herdr (prefix `ctrl+b`, `prefix+?` help, `ctrl+alt` as the safe
  direct family). Override under `keys:` in config. A new action
  adds a name + default before the handler.
