---
name: dev
description: >
  Run, test, and extend anvil (Rust harness + CPython hammer + smith TUI).
  Use when building, striking from the CLI, launching smith, or changing
  the strike protocol.
---

# anvil — run and test

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
- `anvil serve` is not implemented. Don't add a daemon until smith needs
  to attach to an already-running anvil.

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

Enter asks (model → strike). Ctrl+S raw-strikes the compose buffer as
Python. `@` opens a fuzzy file picker (cwd, skips `.git`/`target`).
Alt+. folds the last thinking/strike card. Wheel / PgUp / PgDn scroll.
The ask worker runs off the UI thread so the spinner keeps moving.

## Ask

`anvil complete` is chat. `anvil ask` is the agent: extract Python (reject
bash waffle), strike, retry up to 3 turns on missing code or a failed
strike. Acceptance: `ask::tests::ask_rejects_waffle_and_strikes_python`.
