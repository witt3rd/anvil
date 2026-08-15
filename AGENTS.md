# anvil

A one-tool agent: the model writes Python; work lives outside the prompt.

## Goal

Fixed tool menus (`read_file`, `bash`, `edit`, …) dump every result into the
context window. Compaction then deletes. This repo inverts that: one action,
`strike` — run code in a persistent guest. Intermediate data stays as
variables. Only what the guest **prints** or **returns** enters the prompt.

The first slice is the machine, not the model. You (or later a model) type
Python in **smith**. **anvil** supervises. **hammer** executes. Restart the
hammer; the store survives. The model loop is next, not first.

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
  src/bin/anvil.rs     CLI: strike | serve
  src/bin/smith.rs     TUI
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
./target/debug/smith --store /tmp/anvil-demo
```

`ANVIL_STORE` sets the default store (else `$HOME/.anvil/default`).
`ANVIL_HAMMER` overrides the guest script (else `hammer/hammer.py` next
to the crate).

`anvil serve` is a reserved socket path; v0 smith talks to the harness
in-process and owns the hammer child.

## Git

Primary clone stays on `main`. Work in `anvil.wt/<branch>/` via `git wt-new`.
This founding tree is the exception: it *is* the mainline.

## Caretaker

Leave the repo cleaner than you found it. Facts that orient live here.
How-to and gotchas live in `skills/dev/SKILL.md`.
