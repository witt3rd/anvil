# Kernel

A multiplexer is a text-mode window manager. It sits between a
terminal emulator and processes on PTYs.

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
| **daemon** | Stays up. Owns sessions. Socket for clients. |
| **session** | Named group of windows. Does not run. |
| **window** | One screen. Panes tiled to fill it. Does not run. |
| **pane** | Rectangle. Views a process. Holds the PTY and the character grid. |
| **process** | The running program (a shell, an editor, …). What the daemon keeps alive. |
| **client** | On a tty. Views a session. Sends keys. |

Detach drops the client. Sessions, windows, and panes stay. No `SIGHUP`.
Processes keep running.

The only thing that runs is a process. A pane is how you see it and
type at it. A session is how you find those panes again.

## What it must do

Without all five, it is not a multiplexer.

1. Processes outlive the client. Close the emulator or drop SSH: they
   keep running.
2. PTY in the middle. The daemon holds the master; the process is on
   the slave and thinks it has a screen.
3. A grid in the daemon, per pane. Parse the process's bytes. On
   reattach, paint the client from the grid.
4. Split the screen. Resize the tty, resize the panes, tell the
   processes (`SIGWINCH`).
5. One keyboard. A prefix is a multiplexer command. Anything else
   goes to the focused pane's process.

## This tree

`src/` starts empty. The old program is in `quarantine/`. Copy a
piece when a primitive above requires it. Rewrite it to these words.
