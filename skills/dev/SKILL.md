---
name: dev
description: >
  Build the multiplexer, fault code in from quarantine, and ship a
  binary. Use when compiling, testing, installing, or copying a piece
  of the mothballed program.
---

# Client

Whenever a vim motion is bound, bind the matching arrow too:

| vim | arrow |
|---|---|
| `h` | Left |
| `j` | Down |
| `k` | Up |
| `l` | Right |

A list that needs a selection takes a mouse: click a row to pick
it, wheel to move (`j`/`k`). Enter confirms. Prefix pane focus,
the session popup, the agent catalog, and the native/anvil list
are the current lists.

Prefix `m` is the current window's note. It is a text box, not a
list: enter is a newline, esc saves the markdown blob on the
window. Space on `- [ ]` / `- [x]` toggles the task.

# Build

```bash
cargo test
cargo build --release
```

```bash
cargo run
cargo run -- --restart    # this tree's daemon + client
```

# Git

Iterate on `main`, matching `AGENTS.md`. Small commits, no leftover
dirty tree. Stage only the files this change owns — never `git add
-A`. Push `origin/main`.

A second change that does not touch the same files is a worktree
off `main`. After that work lands, remove the worktree and its
branch together. `git worktree list` is the check.

# Channel

`~/.local/bin/anvil` is a symlink to `scripts/launch`. It execs
`target/release/anvil` from one tree. Git pull does not rebuild
that ELF. Launch does: if HEAD or `src/` is newer than the binary,
it runs `cargo build --release` in the channel's tree. `anvil daemon`
started by systemd does not. `anvil channel show` prints `stale=yes`
when a rebuild would happen. `ANVIL_SKIP_BUILD=1` skips.

```bash
ln -sf "$(pwd)/scripts/launch" ~/.local/bin/anvil
anvil channel show
anvil channel stable                    # this clone (main); builds if stale
anvil channel dev /path/to/worktree     # builds if stale
```

Channel is not git. `~/.anvil/channel` and `~/.anvil/dev-root` remember
the pick. One ELF: the client probes `enumerate.build` (git of this
binary) before attach. A mismatch stops the listener and starts this
build — the systemd unit if it is enabled, else this process detached.
An older daemon with no `build` is a mismatch. Processes that daemon
held end. After a channel switch the next `anvil` does this; you can
also:

```bash
systemctl --user restart anvil
```

# Ship

From a clone, with Rust and a C linker on PATH:

```bash
cargo test
cargo build --release
ln -sf "$(pwd)/scripts/launch" ~/.local/bin/anvil
```

Dependencies must resolve from the network. Path deps cannot ship.

opaline is a git tag, not crates.io:

```
opaline = { git = "https://github.com/hyperb1iss/opaline.git", tag = "v0.4.1", features = ["ratatui"] }
```

If this tree carries a patch that is not upstream, the dep is the
fork and its tag. A local checkout is not a dep.

The daemon is a systemd --user unit. `Type=simple`. SIGTERM stops
it; there is no `anvil daemon stop` subcommand.

```
[Unit]
Description=Anvil — multiplexer daemon for ACP agents and shells
After=default.target

[Service]
Type=simple
ExecStart=%h/.local/bin/anvil daemon
Restart=on-failure
RestartSec=2s
TimeoutStopSec=8s

[Install]
WantedBy=default.target
```

`loginctl enable-linger` on the user so the unit survives logout.
The client (`anvil`) attaches to `$XDG_RUNTIME_DIR/anvil.sock` and
will start a detached daemon if none is listening — that is for a
dev tree. A shipped box holds the daemon with the unit. Opening a
session spawns each named agent pane again (`session.json`
`agents`). The process is new; the pane, the catalog name, and that
pane's inner session id are not. ACP calls `session/load` or
`session/resume` with that id. A native TUI gets the catalog
`resume` argv (`{session}`). Do not use `--continue`: that is the
last conversation on the box, not this pane's.

The rail's turning mark is OpenCode's HTTP door (`--port`). Prefix
`a` / `A` pass that flag. A shell that runs `oc` is adopted: the
session on argv (`-s` / `--session`) is this pane's, and a listen
port anywhere in that process tree is the door. The unit's PATH
must include `~/.local/bin` or spawn cannot find `oc`.

A catalog row is the adapter. Maintain it with `anvil agent`,
not a text editor. Shipped rows live in `agents.default.json`.
The daemon implements doors, not brands. Adding support:

```
anvil agent add NAME --program P [--acp-only] [--acp-program P] [--adopt A] [--http] [--resume 'F']
anvil agent from oc --as my-oc
anvil agent seed
```

Prefix `a` asks which directory, then launches the default native
TUI when they have one, else anvil. The default is this pane's
cwd. Roots (`~/src/witt3rd`, `~/src/li`) appear as they accumulate
from launches. Typing a path lists matching folders in that
directory first, then other folders whose names contain the
typed fragment. A trailing slash lists only that folder. The
window is named after that folder. Prefix `A` picks the agent first; two seats ask
native or anvil. Turning on anvil is an in-flight `session/prompt`.

# Not yet published

Ship is install-from-git. These remain:

- **crates.io.** Not published. The name `anvil` is taken (a
  templating crate). crates.io rejects git dependencies; opaline is
  git. A publish needs a free name and opaline (or a fork) on
  crates.io.
- **Release.** Version is `0.1.0`. No tags.
- **Unit in this tree.** None. `quarantine/systemd/anvil.service`
  speaks the old `anvil serve` CLI. Do not copy it.
- **Install the crate owns.** No `anvil daemon --install`. The
  binary is copied by hand (or by whoever provisions the box).
- **CI.** None.

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
