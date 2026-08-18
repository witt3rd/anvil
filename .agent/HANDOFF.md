# HANDOFF — anvil, primary

_State: charter landed on `main`. Clean. One worktree (primary on
`main`), one branch (`main`)._

## State

- `main` holds the ACP-parent charter. Primary clone clean after
  this land.
- Code unchanged. Tests were green on `f9bda9b`.
- `write { pane }` is documented, not implemented.
- Rail / roster is specified, not drawn.

## What changed

Anvil is the multiplexer for ACP agents and shells. The daemon is
the parent. The sidebar is a rail of marks that opens into a
roster. Write is the courier.

- `AGENTS.md` — the charter.
- `docs/kernel.md` — parent, view, write may name a pane.
- `docs/tui.md` — rail (`◇` / dot pulse / `◆` in `error` red),
  roster on prefix `s`, ACP feeds the marks.
- `docs/daemon.md`, `docs/client.md`, `docs/protocol.md` — aligned.

A2A-on-ACP is specified in the ACP tree (not this repo):
`/home/dt/src/ext/agent-client-protocol.wt/docs--a2a/a2a.md`
on branch `docs/a2a`. Local only; not pushed to upstream ACP.

## Where to pick up

Draw the rail. `read` carries each window's process and a state
the daemon can see. ACP parent (exclusive attach) is how that
state exists for an agent. Named-pane `write` is the courier.

## Gotchas

- **Daemon vs client protocol**: both sides must be the new build.
- **`anvil`** is the stable release via `~/.local/bin/anvil` →
  `scripts/launch`.
- **opaline** path dep: `/home/dt/src/ext/opaline` at `v0.4.1`.
- **PATH**: bash tool lacks `~/.local/bin`.
- Kernel pages stay six words. ACP, roster, rail live in
  `AGENTS.md` and `docs/tui.md`.

## Next (single most important)

Implement the rail: three cells of marks, `read` with process and
state, prefix `s` opens the 42-cell roster.
