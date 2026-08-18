# HANDOFF — anvil, primary

_State: rail and roster are drawn. After land, `main` is clean._

## State

- Charter + `docs/acp.md` + rail chrome.
- Prefix `s` toggles rail (3 cells) and roster (42 cells).
- Prefix `c` names a window (`plugin`, `ui`). The name is the
  window. Roster shows it.
- Marks are idle / dead from `grid.alive`. Turning and needs-you
  wait on ACP parent.

## What changed

The sidebar lists **windows** of the attached session. Default is
the rail (`┃` + `◇`). Prefix `s` opens the roster (name + idle/dead)
when the tty is 120+ wide. Canvas resizes with the toggle.

## Where to pick up

ACP increment (1): daemon holds stdio, derives turning / needs-you,
`read` carries that state so the diamond can go red.

## Gotchas

- **Daemon vs client protocol**: both sides must be the new build.
- **`anvil`** is the stable release via `~/.local/bin/anvil` →
  `scripts/launch`. Rebuild release after land for the live client.
- **opaline** path dep: `/home/dt/src/ext/opaline` at `v0.4.1`.
- Kernel pages stay six words. ACP lives in `docs/acp.md`.

## Next (single most important)

Hold ACP stdio in the daemon and feed the rail `turning` /
`needs_you`.
