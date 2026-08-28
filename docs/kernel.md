# Kernel

A multiplexer is a text-mode window manager. The daemon holds the
processes. The client donates its tty and sends keys. The daemon paints.

**Radical simplicity:** every word below had to earn this page. Do not
add a seventh.

```
daemon
 └── session
       └── window
             └── pane  ──views──▶  process
client  ──attaches──▶  session
```

| Word | What it is |
|---|---|
| **daemon** | Stays up. Parent of every process. Owns sessions. Socket for clients. |
| **session** | Named group of windows. Does not run. |
| **window** | One screen. Panes tiled to fill it. Does not run. |
| **pane** | Rectangle. Views a process. Holds the input, the output, and the view. |
| **process** | The running program. What the daemon keeps alive. |
| **client** | On a tty. Donates that tty. Sends keys. |

Detach drops the client. Sessions, windows, and panes stay. Processes
keep running.

The only thing that runs is a process. A pane is how you see it and
type at it. A session is how you find those panes again. A window is
how you group the panes of one activity.

## What it must do

These five make a multiplexer.

1. Processes outlive the client. Close the emulator or drop SSH: they
   keep running. The process stays on the turn.
2. The daemon is the parent. It holds the process's input and output.
   On a PTY, that is the master; the process is on the slave and
   thinks it has a screen.
3. A view in the daemon, per pane. On a PTY, parse the bytes into a
   grid. While a client is attached, paint that tty from the view.
   On reattach, paint again from the same view.
4. Split the screen. Resize the tty, resize the panes, tell the
   processes (`SIGWINCH`).
5. One keyboard. A prefix is a multiplexer command. Anything else
   goes to the focused pane's process.

The daemon writes to a process. Keys from the focused pane. A write
may name any pane.

## This tree

The kernel lives in `src/`. The old program is in `quarantine/`.
Copy a piece when a primitive above requires it. Rewrite it to
these words.
