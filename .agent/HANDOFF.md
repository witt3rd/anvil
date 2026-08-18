# HANDOFF — anvil, primary

_State: `docs/acp.md` is in this repo. After land, `main` is
clean._

## State

- Charter plus ACP host spec live in-tree.
- Code unchanged. Rail / roster specified, not drawn.
- `write { pane }` documented, not implemented.

## What changed

`docs/acp.md` is the host: ACP used twice, children on stdio,
viewers on the unix socket, roster via `session/list`, rail fed
by `session_info_update`. Pulled across from the leftover ACP
worktree and named ACP.

## Where to pick up

Draw the rail. `read` carries process and state. Exclusive ACP
attach is how that state exists for an agent.

## Gotchas

- **Daemon vs client protocol**: both sides must be the new build.
- **`anvil`** is the stable release via `~/.local/bin/anvil` →
  `scripts/launch`.
- **opaline** path dep: `/home/dt/src/ext/opaline` at `v0.4.1`.
- Kernel pages stay six words. ACP lives in `docs/acp.md`.

## Next (single most important)

Implement the rail: three cells of marks, `read` with process and
state, prefix `s` opens the 42-cell roster.
