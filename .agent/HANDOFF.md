# HANDOFF — anvil, primary

_State: charter rewrite on `docs/charter`. Land this branch so
`main` matches._

## State

- Branch `docs/charter` in `/home/dt/src/witt3rd/anvil.wt/docs--charter`.
- Code unchanged. Tests were green on `main` at `f9bda9b`.

## What changed

The product is written down. Anvil is the multiplexer for ACP
agents and shells. The daemon is the parent. The sidebar is the
roster. Write is the courier.

- `AGENTS.md` — the charter.
- `docs/kernel.md` — parent, view, write may name a pane.
- `docs/tui.md` — roster, tiles, courier. Component architecture
  gone. Which-key and theme stay.
- `docs/daemon.md`, `docs/client.md`, `docs/protocol.md` — aligned.
  `write` with a pane is the same verb; the code still writes to
  the focused pane only.

## Where to pick up

First earned code: make the roster true. `read` carries each
window's process and a state the daemon can see. ACP parent
(exclusive attach) is how that state exists for an agent.
Named-pane `write` is the courier.

## Gotchas

- **Daemon vs client protocol**: both sides must be the new build.
- **`anvil`** is the stable release via `~/.local/bin/anvil` →
  `scripts/launch`.
- **opaline** path dep: `/home/dt/src/ext/opaline` at `v0.4.1`.
- **PATH**: bash tool lacks `~/.local/bin`.
- **`write { pane }` is documented, not implemented.**
- Kernel pages stay six words. ACP, roster, courier live in
  `AGENTS.md` and `docs/tui.md`.

## Next (single most important)

Land `docs/charter` onto `main`. Then implement the roster: `read`
with process and state, sidebar draws windows not only session
names.
