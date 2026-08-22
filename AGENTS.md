# anvil

A multiplexer for ACP agents and shells. Ontology: `docs/kernel.md`.

## Goal

The daemon holds every process on the box. A shell sits on a PTY. An
ACP agent sits on stdio. The client tiles their screens and draws a
roster of what they are doing. SSH reaches the box. Local attach is
a unix socket.

A session is a domain (`personal`, `spire`). A window is one
activity (`ui`, `plugin`, `anvil`): an agent pane and a shell pane.
Ten agents run at once. The sidebar is a rail of marks by default
and opens into a roster for a glance — idle, turning, needs you.
The daemon writes to any pane,
so a note can move from one window's agent to another.

That is the kernel to implement. The old program is in `quarantine/`.
It is reference. Fault in the smallest piece that earns its place.
Rewrite it to these words.

Libraries (PTY, VTE, TUI) are fine. Anvil is its own package.

## Principle

**Radical simplicity.** Every concept must earn its place.
**Zhengming (正名):** right names, common computer terms only.

Six kernel words: **daemon**, **session**, **window**, **pane**,
**process**, **client**. Chrome is the client's draw path
(`docs/tui.md`) — the roster and the tiles. ACP is how a process
talks. If you need another kernel word, stop. The need is not
proven.

The crate is named anvil. That is a path.

## Merits

1. **These words only.** A new noun is a last resort.
2. **Detach never kills.** The process stays on the turn. Restore
   the tty.
3. **Daemon is the parent.** It holds the PTY or the ACP stdio.
   Exclusive ACP attach first. The client is a viewer.
4. **One package.** Anvil is the host.
5. **Unsandboxed.** A process has the same trust as a shell.
6. **SSH is the bus.** Local attach is a unix socket. Mux ops tile
   the screen. ACP talks to an agent process.
7. **The roster is the product.** A rail of marks; it opens into
   the activity list.
8. **The daemon writes.** A prompt can reach another pane.

The daemon is the parent of a process. Their TUI stays the place
you type when that window is focused.

## Concepts

```
daemon
 └── session          domain
       └── window     activity
             └── pane  ──views──▶  process
client  ──attaches──▶  session
```

A pane views a process. Two encodings of the same view: a character
grid (PTY) or an ACP stream (stdio). The human pane is their TUI
when that TUI can attach, or the PTY of that TUI as its own process.
The machine door is ACP, held by the daemon.

The courier is `write`: the daemon already sends bytes to a process.
ACP `session/prompt` is that write for an agent. A further word
earns its place after that write is in use.

## Layout

```
src/               daemon, proto, client
agents.default.json  catalog rows (the adapters)
docs/kernel.md     the six words
docs/tui.md        chrome: rail, roster, tiles
docs/acp.md        ACP: the host, the children, the viewers
docs/protocol.md   the mux wire
quarantine/        mothballed previous tree (do not grow)
skills/dev/        how to build, fault in, and ship
```

## Commands

```bash
cargo test
cargo build --release
cargo run -- --restart    # this tree: stop daemon, start this binary, attach
```

`~/.local/bin/anvil` is `scripts/launch`: it execs `target/release/anvil`
from this clone (stable) or a worktree (dev). `anvil channel` picks.
If that ELF lags the tree, launch rebuilds it (`cargo build --release`).
The client will not attach to another build: `enumerate` carries
`build`, and a mismatch replaces the listener.

To read the old program: `quarantine/`. To run it: that tree has its
own `Cargo.toml`.

## Ship

A clone on any Unix box with Rust and a C linker is enough. Dependencies
come from the network — crates.io or git — never a path on the author's
disk. `cargo test` and `cargo build --release` succeed. PATH is
`scripts/launch`, which execs that release binary. The daemon is
`anvil daemon`; the client is `anvil`. A systemd --user unit holds
the daemon so detach never kills it.

That is ship. Publishing to crates.io, a free crate name, a release
tag, a unit file in this tree, and an install the crate itself owns
are not that yet. The list lives in `skills/dev/`.

## Client

Vim motion keys (`h` `j` `k` `l`) always have the matching arrow.
A list that needs a selection always takes a click, and the wheel.
The pairs and the pickers live in `docs/tui.md`.

## Git

Two modes. A single change iterates on `main`: small commits, no
leftover dirty tree. Push a branch when that change is ready to
merge.

A second change that does not touch the same files is a
worktree. After that merge, remove the worktree and its branch.

Sync with `origin/main` before starting, and before a PR. The
lived commands live in `skills/dev/`.

## Caretaker

A change earns its place by making the multiplexer simpler or more
correct. New words go in `docs/kernel.md` only after they have
earned it. `skills/dev/` is the lived how-to.

## Documentation

**Say what a thing is.** Positive statements. Kernel docs
(`docs/kernel.md` and the docs it owns) use only the six words.
ACP, roster, and write live here, in `docs/acp.md`, and in
`docs/tui.md` until a word earns the kernel page.

This file and `skills/` are public. They describe the program, not
anyone's machines.

## Scope

Universal for anyone working in this tree. Single owner, no
external contributors.

Last updated: 2026-08-22.
