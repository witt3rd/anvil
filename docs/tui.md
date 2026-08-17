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

## Opaline theme integration

The ratatui client can incorporate a theme engine like
[opaline](https://github.com/hyperb1iss/opaline)
for semantic color palettes. Themes define tokens (`bg.base`, `bg.selection`, etc.) that map to
ANSI colors, enabling consistent theming across the application. The client draws immediate-mode
widgets styled from these resolved colors — no widget tree, no retained state.