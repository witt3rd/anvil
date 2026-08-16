---
name: dev
description: >
  Run, test, and extend anvil (Rust harness + CPython hammer + smith TUI).
  Use when building, striking from the CLI, launching smith, or changing
  the strike protocol.
---

# anvil — run and test

Session frame design: `references/session-frame.md`.

Charter: repo-root `AGENTS.md`. Git: house `git` skill — `git wt-new`
from anywhere in the repo; never commit on `~/src/witt3rd/anvil` (`main`).

## Run

```bash
# in a worktree
cargo build --release --bins
anvil channel dev "$(pwd)"
smith -p nim

# back to main
anvil channel stable
```

Debug without the wrapper: `./target/debug/anvil strike '2 + 2'`.

Enter sends a strike. Ctrl+J is a newline. Ctrl+C quits. The hammer
respawns if it exits; the store is the namespace.

## Test

```bash
cargo test
python3 hammer/hammer.py --self-test         # guest only, no rust
anvil prof                                   # fold + last spans (serve if up)
ANVIL_TRACE=1 smith -p nim                   # tracing spans on stderr
```

`persist_across_respawn` in `src/lib.rs` is the load-bearing test: strike
`x = 1`, kill the guest, strike `x` → `1`.

## Protocol

One JSON object per line on the hammer's stdin. Reply, one line, stdout.

```json
{"id":"1","op":"strike","code":"print(1)\n2+2"}
{"id":"1","ok":true,"value":4,"stdout":"1\n","stderr":"","error":null}
```

Other ops: `ping`, `reset` (drop namespace + pickle), `shutdown`.

## Gotchas

- Do not `exec` in the anvil process. The guest is a child.
- Unpicklable names are dropped on persist and listed in `stderr` once.
- A strike that is only statements has `value: null`. Prints still show.
- `anvil serve` owns hammers. smith starts it if the socket is down.
  `anvil serve --status` / `--stop`. `anvil serve --install` enables
  the user systemd unit (back after login/reboot, still cold).
  `--uninstall` removes it. `--install` also `loginctl enable-linger`
  so the user instance (and serve) start at boot. Log: `~/.anvil/serve.log`.
  Close smith:
  serve stays. `--store` is still in-process (no serve).
  `anvil serve --stop` only sends Shutdown on the socket. It must not
  call `systemctl stop`: the unit's ExecStop is this same command, and
  that pair waits until TimeoutStopSec.
- Event log is `sessions/<id>/events.jsonl` (seq, ts, kind). Serve
  writes it. Cards project it. Ask projects only model-visible
  events (user/ask/strike/answer). Thinking, fiber, status stay out.
- `anvil inspect` reports services, fibers, slots. Live if serve is
  up; otherwise cold from disk. Stage slots follow the last exposed
  or struck session. `anvil mount clock` occupies `casing.status`
  (smith shows the time). `anvil unmount dyn-1` stops the fiber.
  Temporary. Gone when serve stops.
- Conceptual objects: `anvil session|workspace|catalog` under
  `$ANVIL_ROOT` (default `~/.anvil`). Destroying a workspace or catalog
  does not destroy its occupants.
- smith without `--store` is a casing: Tab focuses the rail, `n` names
  a session (empty Enter mints a word), `p` names a PTY, `e` names a
  text editor, `c` adds a clock member, `g` pins a diagnose log of
  the focused session. Enter
  exposes a member. First attach warms the front bench. Transcript is
  `sessions/<id>/transcript.jsonl`. `--store` is the old single-pane
  escape hatch.
- Mixed members: `anvil workspace add fleet-os bash --pty` (or
  `anvil pty new bash`). Serve owns `$SHELL` (portable-pty + vt100).
  Focus the pty pane: keys go to the shell; Ctrl+C is SIGINT; Ctrl+Q
  closes the casing. `anvil pty snap bash` prints the live screen
  (warms if cold). `anvil pty write bash echo hi` types a line + Enter.
  `anvil inspect` puts the last rows on `casing.main` when that member
  is front. A bench with two or more members stacks every pane.
  Zellij/herdr/tmux are scars, not hosts.

## Providers

Live file: `~/.config/anvil/config.yaml`. Do not write secrets into the
repo. Shape: `config.example.yaml`.

```bash
anvil providers                  # never prints resolved keys
anvil login grok                 # execs `grok login`
anvil models --refresh
ANVIL_CONFIG=/tmp/t.yaml anvil providers
```

`!` values run `sh -c` on the rest and cache stdout for the process.
Fail loud on nonzero. Provenance: Prime upstream, generic — not a
Doppler feature. Tests use `!printf`, not doppler.

## Smith TUI

```bash
cargo build --bins
./target/debug/smith -p nim
```

Keys follow herdr's default map (`src/tui/keys.rs`, herdr action names).
First five: `prefix+c` new sash, `prefix+v`/`-` split, `prefix+h/j/k/l`
pane, `prefix+w` catalog picker, `prefix+q` detach. Also `prefix+x`
close pane, `prefix+z` zoom, `prefix+r` resize, `prefix+[` copy-scroll,
`prefix+1..9` sash jump, `prefix+s` settings, `prefix+shift+r` reload.
Smith-only verbs are off herdr chords: strike is `ctrl+s`/`prefix+enter`,
new session is rail `n`, trajectory is `prefix+shift+y`, stats is rail `o`
(lifecycle / cache / tok/s / trace of the focused session). Double-esc
(or `ctrl+u`) clears the compose box.
Safe direct chords: `ctrl+alt+][` sash, `ctrl+alt+h/j/k/l` pane,
`ctrl+alt+d` split. On the rail, `hjkl` focus panes, arrows switch
sashes, `[` cycles catalog/workspace/member columns.
Tab still toggles the rail except on a focused PTY. Override under
`keys:` in config.yaml. Enter asks (model → strike). `prefix+s` /
Ctrl+S raw-strikes.
`@` opens a fuzzy file picker (cwd, skips `.git`/`target`).
Alt+. folds the last think/decode/strike card. Alt+V cycles ask verbosity
(quiet / steps / full). Phases match the stats pane: Prefill, Think,
Decode, Tool, same colors. Full streams the reason and decode text
(and step timings) instead of a generic "Thinking" label.
Drag-select copies to the clipboard and toasts (herdr
`ui.copy_on_select`, default on). `ui.copy_on_select: false` keeps the
highlight; Ctrl+C copies. Drag a shared pane border to resize;
double-click the edge to equalize. Paste into ask becomes
`[Pasted: N lines]` or `[Image #N]` with a preview card; paste again
or double-click expands. Ctrl+V pastes a clipboard image.
Wheel / PgUp / PgDn scroll.
Tab focuses the rail. Click a tab, rail row, or pane to focus it.
Wheel scrolls the pane under the cursor (a focused PTY gets up/down;
wheel on a member peeks like `j`/`k`; wheel on a workspace
highlights like arrows; wheel on the sash strip cycles
workspaces). Click `[<]` `[>]` to cycle sashes; `+` names a session.
`prefix+n`/`p` cycle sashes; `prefix+h/j/k/l` cycle members.
`prefix+shift+l` toggles the trajectory sash (event log). A second
`smith` is another casing on the same serve.
`/mount clock` and `/unmount` (`prefix+m` / `prefix+u`;
rail `m`/`u`) mount a temporary clock on `casing.status`. Ctrl+C
closes the casing; serve keeps the hammers. Rail `p` names a PTY
member. When that pane is focused, keys go to `$SHELL` and Ctrl+Q
closes the casing (Ctrl+C is SIGINT).
The ask worker talks to serve so the spinner keeps moving.
Every painted surface has a dotted face (`message.user.field`,
`hint.key`, `tab.active`, …) filled by a pack (`mocha`, `terminal`).
`theme:` in `~/.config/anvil/config.yaml` retints inks or overrides
faces. User messages sit on a raised field; agent messages stay on
the canvas. The sash carries `[<] [>] [catalog]` pills; the bottom
row is the keybind hint bar. Each smith pane has its own status strip (account │ cwd │ git │
model │ context%). Context/git/cwd/model come from that session.
`clock` is the `casing.status` mount and only shows on the focused
pane. `ui.status_auto_hide: true` hides the strip on unfocused
smith panes. Widgets are listed under `ui.status`. Restart smith
to pick up a theme or ui change; serve can stay.

## Ask

`anvil complete` is chat. `anvil ask` is the agent: extract Python (reject
bash waffle), strike, retry up to 3 turns on missing code or a failed
strike. Acceptance: `ask::tests::ask_rejects_waffle_and_strikes_python`.
