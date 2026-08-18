---
name: dev
description: >
  Build the blank multiplexer tree and fault code in from quarantine.
  Use when compiling, testing, or copying a piece of the mothballed
  program.
---

# Build

```bash
cargo test
cargo build --release
```

`src/main.rs` is a ratatui client: init, draw `test`, any key restores the tty.

```bash
cargo run
cargo run -- --restart    # this tree's daemon + client
```

# Fault in

Old code lives in `quarantine/`. It still has its own `Cargo.toml`.

```bash
# read or run the mothballed tree
ls quarantine/src
cargo test --manifest-path quarantine/Cargo.toml
```

Copy the smallest file that implements a kernel primitive. Rewrite it
to the words in `docs/kernel.md`. Do not `include!` quarantine. Do not
add quarantine as a path dependency. If the piece needs a name that is
not daemon, session, window, pane, process, or client, leave it.

# Git

House `git` skill. Never commit on `~/src/witt3rd/anvil` (`main`).
