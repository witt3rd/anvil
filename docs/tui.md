# TUI

How the **client** draws. Kernel words stay in `docs/kernel.md`.

Chrome is the sidebar and the tiles. ACP is how a process talks;
the daemon is its parent. Their TUI is the place you type when
that window is focused.

## Immediate mode

Each frame: read state, write cells. One frame function.

A pane's view is retained in the daemon. The client copies it into
the pane's rectangle.

A frame is:

1. Measure the tty.
2. Draw the sidebar (immediate).
3. Copy each visible pane's view (retained).
4. Draw overlays (immediate).

Prefix keys belong to the multiplexer. Keys that are not prefix go
to the focused pane's process. Shift+Enter is a newline when the
process asked for kitty keys or modifyOtherKeys — OpenCode's
prompt; a plain Enter still submits.

The library is whatever does immediate-mode cells (today, ratatui).

## The sidebar

The sidebar has two widths. It **opens** by default: names and a
clause of state, 21 cells (half the old full width). Drag the
right edge to resize it. Prefix `s` closes it to the **rail**:
three cells of marks. Prefix `s` again restores the last width.

The sidebar is two lists, split the way herdr splits a rail.

**Windows** sit above. Every window of the attached session, in
the order the operator laid them. A window with no agent is a
plain tmux window: a muted `·`, not a diamond. It does not
appear in the list below.

**Agents** sit below. Each entry is one pane whose process was
spawned from the catalog (`oc`, `oc-work`, `grok`). A window
may hold none, one, or several. Selecting an agent focuses
that pane and brings its window forward.

The list is recency: a turning (or needs-you) agent sits at
the top; then the one that stopped last; the oldest idle at
the bottom. When a turn ends, the client bells unless this
terminal is the focused application and that pane is the
one selected. The mark carries state.

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

The **rail** is three cells: `┃` on the current row, then the
mark. Each entry still occupies two rows — the mark on the name
row, the clause row blank — so marks sit on the same rows as the
open names and the list does not jump when it collapses. The sidebar keeps
a blank row above the footer; the panes do not shrink.
The `─` stays on the same split as the open list.
Agents occupy the block below. The column is `bg.base` — no
panel, no names.

The **open sidebar** is 21 cells by default, `bg.panel`, and
lives in `<root>/sidebar.json`. Each entry is two lines.
The first is the mark and the name. The second is a clause:
on a window, what lives there (`oc · shell`, `shell`), or the
agent's activity when the HTTP door has a session title; on
an agent, that same activity (OpenCode's title, grok's
`generated_title` on disk, or the last user line), plus
turning or needs-you when that is true. These are task
summaries, not gerunds we invent — the agent already
named the work. Grok's turn is the `systemd-inhibit`
it holds while working. Headers `windows` and `agents` sit above each
list in `text.dim`, the same as the host. `windows` has a
blank row above and below. `agents` sits on the row under
the divider. The agents block is always there, even when empty.
A `─` between them is a drag handle: pull it to give
one list more of the column. The current row wears the
accent bar and `text.primary`. Other rows are muted.

Keys typed in the tiles go to the focused pane. The sidebar
is clicked, or walked with prefix `]` / `[`.

| gesture | what it does |
|---|---|
| click a window row | focus that window |
| click an agent row | focus that pane |
| drag the `─` | resize the window / agent split |
| drag the right edge | resize the sidebar |
| prefix `w` | show or hide the windows list |
| prefix `s` | pick a session |

`exit` in a shell ends the process and closes the pane. The
last pane of a window takes the window with it.

A click on a tile focuses that pane. A drag that is not a
click selects cells; release copies the text to the clipboard
and a short toast says so. Wheel, drag, and click go to the
pane's process as SGR mouse only when it has asked (DECSET
1000/1002/1003). Shift-drag selects even then. Prefix `]` /
`[` walk windows. Prefix `v` splits the focused pane to the
right; prefix `-` splits it down. Focus moves into the new
pane. Closing a pane gives the leftover tile the space.

A shell that launches a catalog program (`oc`, `oc-work`,
`opencode`, `grok`) is adopted: the pane gets that name and,
when a port is visible, the HTTP watch — the same as prefix `a`.

Prefix `a` opens a new window on the default agent (`agents.json`).
The catalog names the program (`oc`, `oc-work`, `grok`). An
OpenCode wrapper stays argv0; `--hostname` / `--port` are appended
so the daemon can watch `/session/status`. Prefix `A` picks from
the catalog — a vertical list. An agent with a native TUI and an
ACP command then asks which one: their TUI on a PTY, or anvil's
prompt/response viewer. `acp_only` skips that list. Prefix `c`
opens a shell. Prefix `,` renames the current window. Prefix `m`
opens the current window's note: a markdown blob the daemon
stores with that window. Esc saves. A line that is `- [ ]` or
`- [x]` is a task; space on the box checks it. The footer
hints `ctrl-b m` while a window is in view.

`<root>/agents.json` is the catalog — pure config. The shipped
defaults live in `agents.default.json` in this tree; they are
copied when the user's file is missing. Adding an agent is a
row. The daemon has no table of brands.

```json
{
  "default": "oc",
  "agents": [
    {
      "name": "oc",
      "program": "oc",
      "acp_program": "oc acp",
      "adopt": ["oc", "opencode"],
      "door": { "kind": "http" }
    },
    {
      "name": "rung",
      "program": "rung-agent --acp",
      "acp_only": true
    }
  ]
}
```

A row is the adapter. `adopt` is how a shell's process tree is recognized.
`door` is how the rail and the courier see a native TUI: `http` (local
server), `inhibit` (a descendant cmdline), or omit. ACP panes need no
door. Brand names do not belong in the daemon.

Prefix `a` takes the native TUI when the default agent has one.
A row with no `acp_program` and no `acp_only` is native only until
that command is filled in.

Vim motion keys always have the matching arrow: `h`/`←`, `j`/`↓`,
`k`/`↑`, `l`/`→`. A list that needs a selection (the catalog, the
native/anvil list, the sessions popup, the sidebar) always takes
a click. The wheel moves the same way as `j`/`k`. Enter still
confirms.

## The tiles

Tiles sit flush against the chrome — no content gutter.
Each pane's view sits at the geometry the daemon gave it.
A window is one screen: only the current window draws; the
other windows keep their processes in the daemon.

Chrome is quiet. The frame, the tiles, and the rail share
`bg.base`. The only mark of a boundary is a single thin separator
line — `│` beside a column, `─` below a row — drawn in the subtle
border token. The open sidebar column is `bg.panel`.

The **active tile** keeps full brightness and holds the cursor.
Every other tile wears a dark veil: its cells are scaled toward
black by a fixed factor, a plain brightness shift on the colors
already in the buffer. The eye lands on the active tile.

A default window is two panes: the agent and a shell.

A daemon restart cannot keep the old process (the daemon is its
parent). It does spawn each **named** agent pane again from the
catalog — a new HTTP port for OpenCode, the same ACP command for
anvil's viewer. A pane that was only a shell stays empty until
the client puts a shell on it. The operator should not reattach
to a roster of agent windows sitting at a prompt.

## The courier

The daemon writes to a process. A note to another window is that
write, naming the pane. On an ACP process the bytes are a
`session/prompt`. On an OpenCode TUI they go through the HTTP
door (`/tui/append-prompt` then submit, else the current
session's `prompt_async`) so the turn is the same context as
the headful composer. On a shell they are keys.

Prefix `p` opens a loud prompt bar. Enter sends that write to
the focused agent, or the first named agent on the window.

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
| **client** | `Detach`, `Help`, toggle rail / sidebar |
| **session** | `NewSession`, `PickSession`, `SwitchSession(1..9)` |
| **window** | `NewWindow`, `NextWindow`, `PrevWindow`, `CloseWindow`, `Notes` |
| **pane** | `SplitVertical`, `SplitHorizontal`, `ClosePane`, `FocusLeft`, `FocusDown`, `FocusUp`, `FocusRight` |

Prefix, then:

| Key | Action |
|---|---|
| `q` | Detach (the only way out; Esc goes to the pane) |
| `?` | Help |
| `n` | New session (then a loud name prompt) |
| `s` | Pick a session |
| `w` | Toggle the windows list |
| `$` | Rename the session |
| `1..9` | SwitchSession(n) |
| `a` | New agent (default) |
| `A` | Pick an agent |
| `p` | Prompt the agent (same context as the TUI) |
| `m` | The current window's note (a markdown blob) |
| `c` | New window (shell) |
| `,` | Rename the current window |
| `]` | NextWindow |
| `[` | PrevWindow |
| `&` | CloseWindow |
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
The canvas edge keeps no margin. Tiles meet the chrome.

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
| `bg.panel` | the open sidebar column |
| `bg.elevated` | hovered and elevated surfaces |
| `text.primary` | the current window, the focused pane |
| `text.muted` | hints, other rows |
| `accent.primary` | the current row's `┃` and name |
| `error` | the needs-you diamond |
| `border.subtle` | the separator line between tiles |
| `border.focused` | the prefix popup's border |

## Header

The top row. `bg.panel`. A tmux-style chip on the left: the
session number (or its name after prefix `$`), inverted —
`bg.base` on `accent.primary`, one space of pad. Nothing
else until the host, `text.dim`, on the right.

## Footer

The bottom row. `bg.panel`. Hints are only the keys that
apply now: prefix armed, picker, rename, needs-you, a drag,
sidebar open or closed. Each is a bold `text.muted` key
and a `text.dim` `:desc`, centered. A spawn error takes the row
instead. A name prompt paints the whole bar `error` with
`bg.base` text so it cannot be missed. A name that already
exists stays on the prompt and writes the error next to the
draft; Escape cancels.

Prefix `s` opens a centered session list: each row is the
name and what is on it (agents, shells, dead). `j`/`k`
move, enter switches, `n` makes a new one (and asks for a
name), `x` drops the selected session.

## Saturation

How much of the named-agent fleet is turning. Chrome, not
a kernel word. The hole is the product. Digits stay off
the bar.

**Now** is `turning / named panes`. Idle, needs-you, and
dead sit in the denom and leave a hole. Shells do not
count. Zero agents hides the track. Turning is a prompt
in flight (HTTP watch or ACP), not a TUI that happens
to redraw. A 0% fleet is a row of dots — the hole has
to read.

**Over time** is the time-weighted mean, sampled by the
daemon every five seconds so detach does not reset it. The
clock runs only while there is at least one agent. A
brighter dot on the track is the last 24 hours. Lifetime
stays on disk.

The **header** is every session: a single-pixel hairline
between the session chip and the host. The hue is
`accent.secondary` (the cool blue), so it does not fight
the orange chip or needs-you red. Empty cells are dim
dots. The fill starts as those dots and coalesces into
`─`, brightening toward the tip.

**Bands** are fleet size, not saturation. The ramp's
length is how saturated; how many agents is a later
shell, not a fatter bar. Achievements and a board can
hang off the band later; they are not chrome yet.

The daemon writes `<root>/saturation.json`. The client
reads it. No new proto op.
