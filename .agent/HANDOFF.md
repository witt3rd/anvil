# HANDOFF — anvil, primary

_State: main at `0de776a`, pushed to `origin/main`. Clean. One worktree
(primary on `main`), one branch (`main`)._

## State

- `main` = `0de776a` = `origin/main`. Primary clone clean.
- No linked worktrees, no feature branches.
- `cargo test` green (25 lib + 4 integration), `cargo clippy` clean,
  `cargo build --release` builds.
- `~/.anvil/channel` = `stable`. `~/.anvil/dev-root` still points at the
  deleted `docs--uncute` worktree — harmless (unused when stable).

## What changed since last handoff

The rewrite landed and then five follow-up features/fixes stacked on it:

**Chrome and tiling** (from `docs--uncute` — already landed):
- Opencode palette (`themes/opencode.toml`, embedded, loaded via opaline's
  public loader — opaline untouched, no builtin).
- Sidebar, panes, status line, which-key prefix (`ctrl-b`), `]`/`[` window
  switching.

**Gap, tile distinction, and frame** (three more lands):
- The gap is the true inter-tile distance (not doubled), re-read from
  `<root>/tiling.json` on every resize so a client reload applies config.
- The active tile is identified by a dark veil on every other tile — a
  plain brightness shift on the buffer, needing no theme knowledge.
- The frame and the tiles share `bg.base`; the only mark of a boundary
  is a single thin separator line (`│` beside a column, `─` below a row)
  drawn in the `border.subtle` token — no gap band.

**Close and focus** (two more lands):
- `close` op: the operator's explicit kill — SIGHUP on the process (same
  as `destroy`). Closing a pane collapses the split and re-tiles;
  closing a window's only pane takes the window with it. Bound `x`/`w`.
- `focus` op (extended to a pane): directional pane focus via arrows or
  `h/j/k/l` on the prefix. The client computes the neighbor from the
  view's geometry and sends `focus { pane }` to the daemon.
- A startup crash (separator drawing past the buffer with stale
  pre-resize geometry) was fixed: every buffer write is bounds-checked,
  and `refresh()` runs immediately after `resize_tty` so the first draw
  uses post-resize geometry.

## Where to pick up

The client is functional. Natural next pieces (each needs to earn a
place — see `docs/kernel.md`):

- **Per-window last-focused memory**: switching to a multi-pane window
  currently focuses its first pane (the daemon tracks one session-wide
  focus). A per-window focus slot would restore the last focused pane.
- **Transcript / status-line concept**: chrome that earns a kernel-adjacent
  description.
- **Process lifecycle**: spawn vs kill — we have `close` (kill, SIGHUP)
  and `spawn` (live), but `detach`-never-kills is the kernel merit; the
  client should gracefully handle a pane dying (already does: forward
  refreshes, respawns).

The `src/tui/keymap.rs` actions are intentionally trimmed to wire-backed
ops. Adding an op needs a kernel word first (protocol guard).

## Gotchas

- **Daemon vs client protocol**: every wire op requires BOTH sides to be
  the new build. A stale daemon silently fails the new op. The client's
  `ensure_running` spawns a fresh daemon if the socket is dead, and
  refuses to connect to a stranger (different protocol).
- **`anvil` runs the stable release binary** via `~/.local/bin/anvil` →
  `scripts/launch`. After landing, rebuild release here for `anvil` to
  run the new client (rebuild done: `target/release/anvil` at head).
- **opaline is a path dependency** (`/home/dt/src/ext/opaline`, at
  `v0.4.1`, clean). The opencode theme ships **in-repo** (`themes/`),
  not as an opaline builtin.
- **Shell PATH**: the bash tool's PATH lacks `~/.local/bin`, so `anvil`
  is only reachable from interactive shells.

## Next (single most important)

Verify `anvil` in a real terminal (stable channel, new build) — all the
features land here now: separator line, dark veil, `ctrl-b x`/`w`
close, arrows/hjkl focus, `]`/`[` window switch. Then decide the
next earned kernel feature.
