# HANDOFF — anvil, primary

_State: main at `46bf5fb`, pushed to `origin/main`. Clean. One worktree
(primary on `main`), one branch (`main`)._

## State

- `main` = `46bf5fb` = `origin/main`. Primary clone clean.
- No linked worktrees, no feature branches. The `docs--uncute` worktree
  and `docs/uncute` branch were merged (fast-forward) and removed.
- `cargo test` green (19 lib + 4 integration), `cargo clippy` clean,
  `cargo build --release` builds.
- `~/.anvil/channel` = `stable`. `~/.anvil/dev-root` still points at the
  deleted `docs--uncute` worktree — harmless (unused when stable); remove
  it if you switch channels.

## What changed (this landed the rewrite)

The uncute rewrite is on `main` now — the six-word kernel, the real wire
daemon, and the opencode-styled client. Breadcrumbs:

- `docs/kernel.md` — six words: daemon, session, window, pane, process,
  client. Old tree moved to `quarantine/` (reference, not a base).
- `src/daemon/` — real daemon: sessions on disk, PTY panes, one JSON
  request/reply per line over a unix socket (`src/proto.rs`,
  `docs/protocol.md`). `tiling.rs` reads `tiling.json` (gap, re-read on
  resize so a client reload applies config).
- `src/tui/` — the client: opencode palette (`themes/opencode.toml`,
  embedded, loaded via opaline's public loader — opaline itself is
  untouched), sidebar / panes / status line, which-key prefix (`ctrl-b`),
  gap between tiles, dark veil on inactive tiles, one window at a time
  with `]`/`[` switching.

## Where to pick up

- The client is functional but minimal. Natural next pieces (each needs
  to earn a place — see `docs/kernel.md`): a **status-line/transcript**
  concept, **resize/focus** pane ops (grow/shrink/focus aren't on the
  wire yet — only split), and **per-window focus memory** (switching to a
  multi-pane window focuses its first pane).
- `src/tui/keymap.rs` is intentionally trimmed to wire-backed actions.
  Adding an op needs a kernel word first (protocol guard).

## Gotchas

- **Daemon vs client protocol**: adding a wire op (e.g. `focus`) requires
  BOTH sides to be the new build. A stale daemon silently fails the new
  op. Restart the daemon (or let the client spawn a fresh one) after
  protocol changes.
- **`anvil` runs the stable release binary** (`~/src/witt3rd/anvil/target/release/anvil`)
  via `~/.local/bin/anvil` → `scripts/launch`. After landing, rebuild
  release here for `anvil` to run the new client (done: `target/release/anvil`
  is the new client, 12:25:05).
- **opaline is a path dependency** (`/home/dt/src/ext/opaline`, at
  `v0.4.1`, clean). The opencode theme ships **in-repo** (`themes/`),
  not as an opaline builtin.
- **Shell PATH**: the bash tool's PATH lacks `~/.local/bin`, so `anvil`
  is only reachable from interactive shells.

## Next (single most important)

Rebuild/verify `anvil` in a real terminal (stable channel, new client)
and confirm `ctrl-b [` / `]` window switching and the gap render as
intended — then decide the next earned kernel feature (transcript is the
candidate).
