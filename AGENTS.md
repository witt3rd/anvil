# anvil

A terminal multiplexer. Ontology: `docs/kernel.md`.

## Principle

**Radical simplicity.** Every concept must earn its place. No funny
terms. **Zhengming (正名):** right names, common computer terms only.

Six kernel words: **daemon**, **session**, **window**, **pane**,
**process**, **client**. Chrome and the agent are the client’s draw
path (`docs/tui.md`), not more kernel words. If you need another
kernel word, stop. The need is not proven.

The crate is named anvil. That is a path. It is not a seventh word.

## Goal

Implement the kernel from zero. The old program is in `quarantine/`.
It is reference, not a base. Fault in the smallest piece that earns
its place. Rewrite it. Do not revive a name from quarantine.

Libraries (PTY, VTE, TUI) are fine. Other multiplexers (tmux, zellij,
herdr, wezterm) are not dependencies and not hosts.

## Merits

1. **These words only.** A new noun is a last resort.
2. **Detach never kills.** No `SIGHUP` to processes. Restore the tty.
3. **Daemon and client.** The daemon stays up. The client is a viewer.
4. **One package.** Not a plugin in someone else's multiplexer.
5. **Unsandboxed.** A process is a shell. Same trust.
6. **SSH is the bus.** No custom remote protocol. Local attach is a
   unix socket.

## Layout

```
src/main.rs        client: ratatui init, loop, "test"
docs/kernel.md     the six words
docs/tui.md        chrome + agent, immediate mode
quarantine/        mothballed previous tree (do not grow)
skills/dev/        how to build and how to fault in
```

## Commands

```bash
cargo test
cargo build --release
```

To read the old program: `quarantine/`. To run it: that tree has its
own `Cargo.toml`.

## Git

House `git` skill. Primary `~/src/witt3rd/anvil` stays on `main`.
Work: `git wt-new feat/foo`. `origin` is `git@github.com:witt3rd/anvil.git`.

## Caretaker

If it does not make the multiplexer simpler or more correct, it is
the wrong change. New words go in `docs/kernel.md` only after they
have earned it.

## Documentation guidelines

**Never say what something is not — always say what it is.** This document
uses only positive statements. Describing concepts by negating them
obscures their true nature and complicates the ontology. All documentation
must describe concepts using only the six kernel words: daemon, session,
window, pane, process, client. If a concept does not earn its place
within these six, it does not belong in the kernel docs.
