# HANDOFF — anvil, primary

_State: charter is on this history. After merge, `main` is the
new AGENTS.md._

## State

- Docs-only. Code unchanged. Tests were green on `f9bda9b`.
- `write { pane }` is documented, not implemented.

## What changed

Anvil is the multiplexer for ACP agents and shells. The daemon is
the parent. The sidebar is the roster. Write is the courier.

See `AGENTS.md`, `docs/kernel.md`, `docs/tui.md`.

## Where to pick up

Make the roster true. `read` carries each window's process and a
state the daemon can see. ACP parent (exclusive attach) is how
that state exists for an agent. Named-pane `write` is the courier.

## Gotchas

- **Daemon vs client protocol**: both sides must be the new build.
- **`anvil`** is the stable release via `~/.local/bin/anvil` →
  `scripts/launch`.
- **opaline** path dep: `/home/dt/src/ext/opaline` at `v0.4.1`.
- **PATH**: bash tool lacks `~/.local/bin`.
- Kernel pages stay six words. ACP, roster, courier live in
  `AGENTS.md` and `docs/tui.md`.

## Next (single most important)

Implement the roster: `read` with process and state, sidebar draws
windows, not only session names.
