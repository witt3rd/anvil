---
name: dev
description: >
  Run, test, and extend anvil (Rust harness + CPython hammer + smith TUI).
  Use when building, striking from the CLI, launching smith, or changing
  the strike protocol.
---

# anvil — run and test

Charter: repo-root `AGENTS.md`.

## Run

```bash
cargo build --bins
./target/debug/anvil strike '2 + 2'          # value 4
./target/debug/smith                         # TUI, store ~/.anvil/default
```

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

`!` values run `sh -c` and cache stdout for the process. Fail loud on
nonzero. Tests for this do not call doppler — they use `!printf`.

## Ask

`anvil complete` is chat. `anvil ask` is the agent: extract Python (reject
bash waffle), strike, retry up to 3 turns on missing code or a failed
strike. Acceptance: `ask::tests::ask_rejects_waffle_and_strikes_python`.
