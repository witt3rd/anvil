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

Status line, sidebar, transcript, prompt: ordinary English for pieces
of chrome UI. They have not earned a place next to daemon and pane.
Add them when we build them.

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

## Opaline theme integration

The ratatui client can incorporate a theme engine like
[opaline](https://github.com/hyperb1iss/opaline)
for semantic color palettes. Themes define tokens (`bg.base`, `bg.selection`, etc.) that map to
ANSI colors, enabling consistent theming across the application. The client draws immediate-mode
widgets styled from these resolved colors — no widget tree, no retained state.