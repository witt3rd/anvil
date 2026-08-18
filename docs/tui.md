# TUI

How the **client** draws. Kernel words stay in `docs/kernel.md`.

Chrome is the roster and the tiles. ACP is how a process talks;
the daemon is its parent. Their TUI is the place you type when
that window is focused.

## Immediate mode

Each frame: read state, write cells. One frame function.

A pane's view is retained in the daemon. The client copies it into
the pane's rectangle.

A frame is:

1. Measure the tty.
2. Draw the roster (immediate).
3. Copy each visible pane's view (retained).
4. Draw overlays (immediate).

Prefix keys belong to the multiplexer. Keys that are not prefix go
to the focused pane's process.

The library is whatever does immediate-mode cells (today, ratatui).

## The roster

The sidebar has two widths. The **rail** is the default: a few
cells of marks, enough to see the fleet without taking the
tiles. The **roster** is the same list opened out — names, a
quiet clause of state — so a glance is enough.

Both widths list the windows of the attached session, in the
order the operator laid them. The list does not sort itself.
The mark carries state.

| state | mark | how the daemon knows |
|---|---|---|
| idle | `◇` hollow diamond, muted | no in-flight prompt |
| turning | `⋅ : ⸬ ⁙` a slow dot pulse, muted | a prompt has not returned |
| needs you | `◆` filled diamond, `error`, a slow pulse | unanswered permission or question |
| dead | `◇` muted | the process has exited |

These are the Grok Build dashboard marks (`diamond_hollow`,
`dot_spinner_frames`, `diamond_filled`). One cell each. Idle is
almost invisible. Turning breathes in the gray. Needs-you is the
only loud row: the `error` token (red), not the warm accent. The
accent is "you are here." Red is "this one is waiting on you."

ACP keeps the rail alive (`docs/acp.md`). The host already holds each child's
stdio. `session/update` is turning. `session/prompt` returning is
idle. `session/request_permission` and elicitation are needs-you.
The host pushes `session_info_update` when the mark must change.
The client does not poll. A PTY child has a thinner signal: alive
or dead, and whatever the process writes to the title.

The **rail** is three cells: the current window's `┃` (`accent.primary`)
and one mark per window. The column is `bg.base` — no panel, no
names. It shows on terminals of 80 cells and wider.

The **roster** is 42 cells, `bg.panel`. Each row is the mark and
the window name. The mark is the state; the name is the activity.
The current row wears the accent bar and `text.primary`. Other
rows are muted.
It shows on terminals of 120 cells and wider; narrower terminals
keep the rail.

Prefix then `s` toggles rail and roster. The tiles resize. The
rail is the rest state. The roster is a look, then it goes back.

Prefix then `c` asks for a window name. Type `plugin`, enter. The
name is the window. The roster shows it.

Prefix then `a` puts an ACP child on the focused pane (`ANVIL_ACP`,
or `opencode acp`). Type a prompt, enter. The rail turns. A red
diamond is needs-you: `y` allows, `n` denies. Prefix then `r` renames
the current window. Prefix then `a` spawns an ACP process on the
focused pane (`ANVIL_ACP`, default `opencode acp`). Enter in that
pane sends `session/prompt`. The rail turns and goes red from
that stream.

The first list on the wire is still the sessions (the domains).
The same column fills with windows once `read` carries each
process and its state.

Jump is a row: `]` / `[` walk windows. On the rail the marks
are the rows. The eye goes where the light is.

## The tiles

The content area has a 2-cell gutter on each side. Each pane's
view sits at the geometry the daemon gave it. A window is one
screen: only the current window draws; the other windows keep
their processes in the daemon.

Chrome is quiet. The frame, the tiles, and the rail share
`bg.base`. The only mark of a boundary is a single thin separator
line — `│` beside a column, `─` below a row — drawn in the subtle
border token. The open roster column is `bg.panel`.

The **active tile** keeps full brightness and holds the cursor.
Every other tile wears a dark veil: its cells are scaled toward
black by a fixed factor, a plain brightness shift on the colors
already in the buffer. The eye lands on the active tile.

A default window is two panes: the agent and a shell.

## The courier

The daemon writes to a process. A note to another window is that
write, naming the pane. On an ACP process the bytes are a
`session/prompt`. On a PTY they are keys.

The prefix picks the window. The text becomes the write. A further
word earns its place after that write is in use.

## Which-key

The client uses [ratatui-which-key] for keyboard input. Every
keypress goes through `handle_key`, which resolves bindings in the
current scope and returns a typed `Action`.

[ratatui-which-key]: https://docs.rs/ratatui-which-key

`Ctrl+B` is the prefix. It arms prefix mode and shows the popup.
The next key dispatches. Escape cancels.

Two scopes:

- **Global** — keys pass through to the focused pane.
- **Prefix** — multiplexer commands. After dispatch, scope returns
  to Global.

Actions are the kernel-only subset — ops that map to the six words
and the documented wire. The set in `src/tui/keymap.rs`:

| Kernel word | Actions |
|---|---|
| **client** | `Detach`, `Help`, toggle rail / roster |
| **session** | `NewSession`, `SwitchSession(1..9)` |
| **window** | `NewWindow`, `NextWindow`, `PrevWindow`, `CloseWindow` |
| **pane** | `SplitVertical`, `SplitHorizontal`, `ClosePane`, `FocusLeft`, `FocusDown`, `FocusUp`, `FocusRight` |

Prefix, then:

| Key | Action |
|---|---|
| `q` | Detach |
| `?` | Help |
| `n` | NewSession |
| `s` | Toggle the rail and the roster |
| `1..9` | SwitchSession(n) |
| `c` | NewWindow |
| `]` | NextWindow |
| `[` | PrevWindow |
| `w` | CloseWindow |
| `v` | SplitVertical |
| `-` | SplitHorizontal |
| `x` | ClosePane |
| `h` / `<Left>` | FocusLeft |
| `j` / `<Down>` | FocusDown |
| `k` / `<Up>` | FocusUp |
| `l` / `<Right>` | FocusRight |

```
Ctrl+B  →  set_scope(Prefix), toggle popup
Key      →  handle_key() resolves binding
           ↓ found: dispatch action, dismiss, return to Global
Escape   →  dismiss, return to Global
```

The which-key popup is the one retained widget in the client — a
transient overlay, written to the buffer each frame.

## The gap

The **gap** is a tiling value in the daemon. It is the distance
between two adjacent tiles, in cells — one value. It is the space
the separator line draws in. The daemon threads it through every
split and resize, so a pane's process sees a PTY sized to the tile.
The canvas edge keeps no margin; the client's content gutter holds
it.

Tiling values live at `<root>/tiling.json`:

    {"gap": 2}

A `gap` of 0 is tiles that abut one another.

## Theme

The ratatui client ships the `opencode` palette in
`themes/opencode.toml` and embeds it at compile time, loading it
through [opaline](https://github.com/hyperb1iss/opaline)'s public
loader. The client names tokens, never hex values. opaline itself
is untouched.

| token | use |
|---|---|
| `bg.base` | the frame and each tile's ground |
| `bg.panel` | the open roster column |
| `bg.elevated` | hovered and elevated surfaces |
| `text.primary` | the current window, the focused pane |
| `text.muted` | hints, other rows |
| `accent.primary` | the current row's `┃` and name |
| `error` | the needs-you diamond |
| `border.subtle` | the separator line between tiles |
| `border.focused` | the prefix popup's border |

## Status line

The bottom row. The session name and the current window sit on the
left. The focused pane's process state sits next to them. Key hints
sit on the right, in the muted text.
