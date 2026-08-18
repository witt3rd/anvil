# HANDOFF — anvil, primary

_State: rail + named windows on `feat/rail`. Live daemon is that
tree's debug binary on `/run/user/1000/anvil.sock`._

## State

- Daemon: `feat--rail/target/debug/anvil` (pid in
  `/run/user/1000/anvil.pid`). Uses window names.
- `~/.anvil/main/session.json` still has windows `1,4,7,…,27`
  from the old numbered daemon. Rename them: `ctrl-b r`.
- New windows: `ctrl-b c`, type a name, enter.

## What changed

Rail of diamonds. Roster is mark + name. First window takes the
session name. `rename { window }` names an existing window.

## Where to pick up

ACP increment (1): daemon holds stdio, `turning` / `needs_you` on
the rail.

## Gotchas

- A stale daemon on the socket ignores window names. Both sides
  must be this build.
- Stopping the daemon closes PTYs (`SIGHUP`).
- **`anvil`** via `~/.local/bin` is still the stable release.

## Next (single most important)

Try `ctrl-b a` on a pane (`ANVIL_ACP` or `opencode acp`). Type,
enter. Rail should turn; red diamond is `y`/`n`. Then land
`feat/rail` when it feels usable.
