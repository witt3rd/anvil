# anvil

A one-tool agent: the model writes Python; work lives outside the prompt.

## Goal

Fixed tool menus (`read_file`, `bash`, `edit`, …) dump every result into the
context window. Compaction then deletes. This repo inverts that: one action,
`strike` — run code in a persistent guest. Intermediate data stays as
variables. Only what the guest **prints** or **returns** enters the prompt.

You type Python in **smith**, or a model will. **anvil** supervises.
**hammer** executes. Restart the hammer; the store survives.

The model is a named provider in `~/.config/anvil/config.yaml`. There is
no first-class enum of vendors. A strike is still the only tool. The LLM
is how the smith *decides* what to strike.

## Merits

1. **Two processes.** The guest will die. If it is also the agent, the
   session dies with it. anvil (Rust) stays up; hammer (CPython) is
   replaceable.
2. **Stock CPython, not IPython.** No Jupyter protocol, no magics, no
   `In[12]`. One JSON line in, one JSON line out.
3. **The prompt is an I/O channel.** Namespace, files, and job output live
   in the store. A strike's `stdout` / `value` / `error` are the only
   egress.
4. **Unsandboxed on purpose.** A strike is the user's (or model's) hands
   on this machine. Same trust as a shell. Do not pretend otherwise.
5. **Names are jobs, not flavor.** Do not add `forge`, `apprentice`, or
   Matrix jokes as process names.
6. **Providers are data.** YAML entries, equal. Do not grow a MultiProvider.
7. **Secrets stay out of the binary and out of logs.** Resolve
   `!doppler …` / `$ENV` at use. Never print the resolved value.
8. **We do not implement OAuth.** Vendor login is the vendor's CLI
   (`grok login`). Cached creds stay where the vendor put them
   (`~/.grok/auth.json`).

## Concepts

| Word | What it is |
|---|---|
| **smith** | TUI. The person at the block. Binary: `smith`. |
| **anvil** | Rust harness. Does not move. Binary: `anvil`. |
| **hammer** | Stock CPython guest. Hits the work. Dies. We hang another. |
| **strike** | One `eval`. A blow, not a process. |
| **store** | On-disk workspace (`namespace.pkl`). Not "the bench." |

Subagents are more smiths, each with their own anvil. No apprentices.

`forge` is taken (a hermes gateway). This repo is **anvil**.

## Layout

```
crates live in this package (one Cargo.toml, two bins)
  src/lib.rs           harness: spawn hammer, strike, respawn
  src/protocol.rs      newline-JSON types
  src/secret.rs        !command / $ENV / literal
  src/config.rs        named YAML providers
  src/oauth.rs         vendor login (grok)
  src/catalog.rs       /models + cache
  src/complete.rs      chat/completions smoke
  src/ask.rs           model → extract Python → strike
  src/bin/anvil.rs     CLI
  src/tui/             smith TUI (blocks, worker, @ picker)
  src/bin/smith.rs     TUI binary
hammer/hammer.py       guest
skills/dev/SKILL.md    how to run and test
```

## Commands

```bash
cargo test
cargo build --bins
./target/debug/anvil strike 'print(2+2)'
./target/debug/anvil strike --store /tmp/anvil-demo 'x = 1'
./target/debug/anvil strike --store /tmp/anvil-demo 'x + 1'   # value 2
./target/debug/smith                         # TUI: Enter ask, @ files, Ctrl+S raw strike
./target/debug/smith -p nim --store /tmp/anvil-demo
```

`ANVIL_STORE` sets the default store (else `$HOME/.anvil/default`).
`ANVIL_HAMMER` overrides the guest script (else `hammer/hammer.py` next
to the crate).

`anvil serve` is a reserved socket path; v0 smith talks to the harness
in-process and owns the hammer child.

## Providers

Config: `~/.config/anvil/config.yaml` (override `ANVIL_CONFIG`).
Example: `config.example.yaml`.

```bash
anvil providers
anvil login grok                 # oauth only; runs `grok login`
anvil models                     # cached /models, refresh if stale
anvil models --refresh grok
anvil complete -p omni 'say hi'   # HTTP only — will waffle
anvil ask -p nim 'how many files have synlinks ~/dotfiles/ (recursive)'
# ask: model writes Python → strike → print stdout. complete does not strike.
```

A secret field is one of:

| Form | Meaning |
|---|---|
| `sk-…` | literal |
| `$NAME` / `${NAME}` | environment |
| `!doppler secrets get KEY -p proj -c cfg --plain` | shell; trimmed stdout |

Bare words are **not** env lookups (Prime does that; it will steal a key
that happens to match an env name).

Model lists: `GET {base_url}/models`, cached at
`~/.cache/anvil/models/<name>.json` for 24h.

jcode's LLM stack is the anti-exemplar except for two moves: named
OpenAI-compatible profiles, and grok-build login = run `grok login` then
trust `~/.grok/auth.json`. We kept those. We did not keep 40 provider
enums, the OpenRouter slot, or env-file key sprawl. Grok Build's *chat*
path in jcode is ACP (a whole agent). anvil talks HTTP
`/v1/chat/completions` for completions. The grok oauth vendor only
supplies the token.

## Git

Primary clone stays on `main`. Work in `anvil.wt/<branch>/` via `git wt-new`.
This founding tree is the exception: it *is* the mainline.

## Caretaker

Leave the repo cleaner than you found it. Facts that orient live here.
How-to and gotchas live in `skills/dev/SKILL.md`.
