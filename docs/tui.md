# TUI

How the **client** draws. Not kernel. Kernel words stay in
`docs/kernel.md`.

## Immediate mode

We draw in **immediate mode**. Each frame: read state, write cells.
No widget tree. No plugin graph. No retained UI objects.

That is the same paradigm for all pieces. One frame function.
Chrome is not a special toolkit. Rich UI is not a special toolkit.

The exception is a **pane**. A process has a character grid in the
daemon. That grid is retained. The client copies it into the pane’s
rectangle. We do not rebuild `bash` from widgets.

So a frame is:

1. Measure the tty.
2. Draw chrome (immediate).
3. Copy each visible pane’s grid (retained).
4. Draw rich UI (immediate).

Prefix keys still belong to the multiplexer. Keys that are not
prefix go to the focused pane’s process.

## What is not a word yet

The chrome section describes the status line and the sidebar as they
exist. Transcript and prompt stay ordinary English until they are
built. Chrome pieces keep their ordinary names; the kernel words in
`docs/kernel.md` stay six.

The library is whatever does immediate-mode cells (today that is
ratatui, when we fault it in). Other multiplexers are still not
hosts.

## Component architecture

Anvil uses the [component architecture] pattern from the ratatui website.
The application is composed of independent **components**, each encapsulating its own
state, event handlers, and rendering logic. This addresses the need to add and extend
functionality without tearing down the entire app — a common problem with ad hoc
arrangements where every new feature risks breaking existing behavior.

The `Component` trait provides four methods:

1. **`init(&mut self) -> Result<()>`** — Set up initial state or resources.
2. **`handle_events(&mut self, event: Option<Event>) -> Action`** — Handle user input,
   returning an `Action` that the app loop processes.
3. **`update(&mut self, action: Action) -> Action`** — Update internal state in response
   to an action.
4. **`render(&mut self, f: &mut Frame, rect: Rect)`** — Render the component's UI given a
   rendering area.

Components are composed at the top level. The application struct holds a tree of components,
and the main loop delegates `handle_events`, `update`, and `render` to the composite. This
means:

- **Adding a feature** = add a new component (or extend an existing one), without modifying
  existing component code.
- **Extending a component** = add handlers or state within its own scope; other components
  are unaffected.
- **Changing huge swaths** = swap out a component's internal logic or even replace an
  entire subsystem (e.g., replace the input component) because the trait contract stays stable.

The library still does immediate-mode rendering — each component's `render` function draws
directly to its rectangle with no retained widget tree — but the *structure* of the app is
now modular rather than ad hoc.

[component architecture]:
  https://github.com/ratatui/ratatui-website/blob/main/src/content/docs/concepts/application-patterns/component-architecture.md

## Which-key input handling

The client uses [ratatui-which-key] for all keyboard input. It owns the
event loop: every keypress goes through `handle_key`, which resolves
bindings in the current scope and returns a typed `Action`.

[ratatui-which-key]: https://docs.rs/ratatui-which-key

### Prefix key

`Ctrl+B` is the prefix key (kernel: "A prefix is a multiplexer
command"). Pressing it arms prefix mode and shows the which-key popup.
The next key dispatches the action. Escape cancels.

Non-prefix keys go to the focused pane's process (kernel: "Anything
else goes to the focused pane's process").

### Scopes

Two scopes control key routing:

- **Global** — normal mode. No bindings. All keys pass through to the
  focused pane.
- **Prefix** — armed after `Ctrl+B`. Bindings here are multiplexer
  commands. After dispatch, scope returns to Global.

### Actions

Actions are the kernel-only subset — only operations that map to the
six words. The full set in `src/tui/keymap.rs`:

| Kernel word | Actions |
|---|---|
| **client** | `Detach`, `Help`, `ReloadConfig` |
| **session** | `NewSession`, `SwitchSession(1..9)` |
| **window** | `NewWindow`, `CloseWindow`, `NextWindow`, `PrevWindow` |
| **pane** | `SplitVertical`, `SplitHorizontal`, `ClosePane`, `FocusLeft/Down/Up/Right`, `SwapLeft/Down/Up/Right`, `CycleNext`, `Zoom`, `GrowPane`, `ShrinkPane` |
| **process** | `PageUp`, `PageDown` |

### Default keybinds

All binds are in prefix mode (`Ctrl+B` then...):

| Key | Action | Category |
|---|---|---|
| `q` | Detach | Session |
| `?` | Help | Session |
| `r` | ReloadConfig | Session |
| `n` | NewSession | Session |
| `1..9` | SwitchSession(n) | Session |
| `c` | NewWindow | Window |
| `w` | CloseWindow | Window |
| `]` | NextWindow | Window |
| `[` | PrevWindow | Window |
| `v` | SplitVertical | Pane |
| `-` | SplitHorizontal | Pane |
| `x` | ClosePane | Pane |
| `h/j/k/l` | Focus pane | Pane |
| `s` → `h/j/k/l` | Swap pane | Pane |
| `Tab` | CycleNext | Pane |
| `z` | Zoom | Pane |
| `=` / `+` | Grow/Shrink pane | Pane |
| `PageUp/Down` | Page scroll | Process |

The `s` prefix shows a sub-group in the which-key popup: `sh` (swap
left), `sj` (swap down), `sk` (swap up), `sl` (swap right).

### Event flow

```
Ctrl+B  →  set_scope(Prefix), toggle popup
Key      →  handle_key() resolves binding
           ↓ found: dispatch action, dismiss, return to Global
           ↓ not found: dismiss (or catch_all)
Escape   →  dismiss, return to Global
Number   →  SwitchSession(n) (handled outside which-key)
```

### Rendering

The which-key popup renders as an overlay on top of the chrome. The
`WhichKey` widget writes directly to the frame's buffer, clearing and
redrawing the popup area each frame. The popup is only visible when
`active` is true or a partial key sequence is pending.

## The chrome

The client draws the opencode TUI proportions: a session list on the
left, the panes in the middle, one status line at the bottom.

- The **session list** is a column of 42 cells. It shows on terminals
  of 120 cells and wider; narrower terminals show only the panes.
- The **content** area has a 2-cell gutter on each side. Each pane's
  grid sits at the geometry the daemon gave it.
- The **status line** is the bottom row. The session name and its
  focused pane sit on the left; the key hints sit on the right, in the
  muted text.

Chrome is quiet. Backgrounds step from the base to the panel; the
attached session in the list wears the accent border and text; all
other text is muted. The pane's grid fills its rectangle with its own
content; the chrome stays in the gray steps and the accent.

## Theme integration

The ratatui client uses the `opencode` builtin theme
([opaline](https://github.com/hyperb1iss/opaline)): semantic tokens
that resolve to colors. The client names tokens, never hex values.

The tokens the chrome uses:

| token | use |
|---|---|
| `bg.base` | the whole frame |
| `bg.panel` | the session list column |
| `bg.elevated` | hovered and elevated surfaces |
| `text.primary` | the session name, the focused pane |
| `text.muted` | hints, other sessions |
| `accent.primary` | the attached session's border and name |
| `border.focused` | the prefix popup's border |

The prefix (`ctrl-b`) arms the action list; the keys that follow are
the documented wire ops. The prefix consumes its keys; every other key
goes to the focused pane's process. `esc` detaches; the session stays.

The client reads the panes' grids from the daemon and copies them at
their geometry. The prefix popup is the one retained widget in the
client — a transient overlay, immediate mode everywhere else.