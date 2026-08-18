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

The sidebar is the activity list. It is a column of 42 cells, shown
on terminals of 120 cells and wider. Narrower terminals show only
the tiles.

Each row is a window of the attached session: the window's name,
the process in its agent pane, and that process's state — idle,
turning, needs you. The current window wears the accent. The
other rows are muted.

The first roster on the wire is the session list (the domains the
daemon owns). The same column fills with windows once `read`
carries their processes and state.

State comes from the stream the daemon already holds. For an ACP
process that is `session/update` and `request_permission`. For a
PTY process that is the grid and whether the process is alive.

Jump is a roster row: `]` / `[` walk windows. The eye goes where
the light is.

## The tiles

The content area has a 2-cell gutter on each side. Each pane's
view sits at the geometry the daemon gave it. A window is one
screen: only the current window draws; the other windows keep
their processes in the daemon.

Chrome is quiet. The frame and the tiles share `bg.base`. The only
mark of a boundary is a single thin separator line — `│` beside a
column, `─` below a row — drawn in the subtle border token. The
roster column is `bg.panel`.

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
| **client** | `Detach`, `Help` |
| **session** | `NewSession`, `SwitchSession(1..9)` |
| **window** | `NewWindow`, `NextWindow`, `PrevWindow`, `CloseWindow` |
| **pane** | `SplitVertical`, `SplitHorizontal`, `ClosePane`, `FocusLeft`, `FocusDown`, `FocusUp`, `FocusRight` |

Prefix, then:

| Key | Action |
|---|---|
| `q` | Detach |
| `?` | Help |
| `n` | NewSession |
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
| `bg.panel` | the roster column |
| `bg.elevated` | hovered and elevated surfaces |
| `text.primary` | the current window, the focused pane |
| `text.muted` | hints, other rows |
| `accent.primary` | the current row's border and name |
| `border.subtle` | the separator line between tiles |
| `border.focused` | the prefix popup's border |

## Status line

The bottom row. The session name and the current window sit on the
left. The focused pane's process state sits next to them. Key hints
sit on the right, in the muted text.
