# anvil

**smith** is who you sit with. This repo is named for the block smith
stands at. One operator (Donald, on roger and the fleet). The model
writes Python. Work lives outside the prompt. Not a category — a daily
seat.

## Goal

Two seats, one binary.

**Focus now** is the small seat: `smith --experience window`. A
**window** (fiber on the tty) holds **panels** in **slots**. A panel is
text, a **terminal** (injects a shell **service** on serve), or
**rows** (more slots — a list of any panels). `prefix+q` unmounts the
window; serve keeps the terminal. Grow by hanging another panel when a
day in this seat requires it. Do not invent column layouts, tabs, or
chrome rails until a bruise earns them. Ontology: `docs/kernel.md`.

The older seat (`smith` with no flag) is the full TUI: rail, tiles,
ask/strike, stats. It stays. We are not growing it first. We are
living in the window and demand-paging back toward an agent mux
(other people's agents as services; our ask/strike stays in the
pocket until we need it).

Fixed tool menus dump every result into the context window. Compaction
then deletes. When we do talk to a model, it does not pick from a
menu. It writes Python. anvil **strikes** it on the hammer.
Intermediate data stays as variables. Only what the guest **prints**
or **returns** comes back.

The LLM does not *use tools*. It *decides what to strike*.

We grow by **radical dogfooding** and **demand paging**. We do not
build a platform in advance of a bruise.

The operator is the most demanding user (Raymond: scratch your own
itch). Taste is the spec, not a committee (auteur). The menu is
curated — every dependency and keybinding is a chef's choice; others
may eat it or walk away (DHH: omakase; **Omarchy** is that palate as a
Linux daily driver). smith should feel like one pair of hands made it.

We have sat in other people's shops: Claude Code, Hermes, Prime, OpenCode,
pi, jcode, tmux, herdr, Zellij. We keep a contract or a scar from each.
We do not become them. Libraries yes; their *system* as host or
dependency, no.

## Merits

1. **One package, one seat.** You type `smith`. anvil and hammer are not
   products you switch to. OSS libraries (ratatui, serde, ureq, later a
   PTY/VTE crate) are fine. Third-party *systems* (zellij, herdr, tmux,
   jcode, Prime) are not dependencies and not hosts. smith is not a
   plugin, not Agent #24, not a tile in someone else's mux.
2. **Two processes.** The guest will die. If it is also the agent, the
   session dies with it. anvil (Rust) stays up; hammer (CPython) is
   replaceable. The store is on disk.
3. **Stock CPython, not IPython.** No Jupyter protocol, no magics, no
   `In[12]`. One JSON line in, one JSON line out.
4. **The prompt is an I/O channel.** Namespace, files, and job output
   live in the store. A strike's `stdout` / `value` / `error` are the
   only egress. Waffle is not an answer.
5. **Unsandboxed on purpose.** A strike is the operator's (or model's)
   hands on this machine. Same trust as a shell. Do not pretend
   otherwise.
6. **Providers are data.** Named entries in YAML, equal. Do not grow a
   MultiProvider or a first-class vendor enum.
7. **Secrets stay out of the binary and out of logs.** A leading `!`
   means `sh -c` the rest (trimmed stdout). `$NAME` / `${NAME}` is env.
   Bare words are literals, not env names. Never print the resolved
   value. This is Prime's upstream contract (`resolve-config-value.ts`,
   Mario Zechner, on `PrimeIntellect-ai/prime-agent`), not a fork patch
   and not Doppler-specific. `!doppler …` is just a command we happen
   to write.
8. **We do not implement OAuth.** Vendor login is the vendor's CLI
   (`grok login`). Cached creds stay where the vendor put them.
9. **SSH is the inter-machine bus.** sshd brokers; our binary is the
   command (`ssh prince smith`). No HTTP/WS remote, no pairing tokens,
   no anvil-specific wire. Local attach is a unix socket first.
10. **Names are jobs.** smith, anvil, hammer, strike, store. Do not add
    `forge`, `apprentice`, or Matrix process names.

## Shape (envisioned)

```
                    ┌─ smith (TUI, ratatui) ─────────┐
                    │  you / thinking / strike / answer │
                    │  @ files · Enter ask · Ctrl+S raw │
                    └────────────┬──────────────────────┘
                                 │ unix socket  (later)
                                 │ ssh stdio    (later: other boxes)
                    ┌────────────▼──────────────────────┐
                    │  anvil (Rust harness)             │
                    │  providers · ask · tiles · serve  │
                    │            │ newline-JSON         │
                    │            ▼                      │
                    │  hammer (stock CPython)           │
                    │  exec · persist namespace.pkl     │
                    └───────────────────────────────────┘
```

**Now:** `smith --experience window` is the growth seat (window →
panel → terminal; a list is a panel of row-slots). Serve owns the
terminal service; detach does not kill the shell. The default `smith`
seat still launches the older TUI. `anvil serve` owns hammers and
shells on a unix socket. Scar: Cordis (service persists, fiber
occupies, slot is a seat). Not a host.

**When use bruises us, in this order:**

1. **Window kernel** — `docs/kernel.md`. Hang panels. Rows only.
   Restore the tty on unmount. Do not smuggle rail geometry.
2. **Services as a list of panels** — inspect feeds row-slots; a row
   can later be any panel, not only text.
3. **Ask/strike as a service a panel injects** — already built
   (`src/ask.rs`, hammer); not hung on the window yet.
4. **Other agents as services** — same injection as a terminal.
5. The older TUI (`src/tui/mod.rs`, `src/frame/`) is a working seat,
   not the place to add the next noun.

**Neighbors (stay neighbors):**

| Tree | Role |
|---|---|
| herdr (`~/src/ext/herdr`) | The desktop mux. smith may run *in* a herdr pane as a process. herdr must not own anvil. |
| zellij (`~/src/ext/zellij`) | Textbook for tiles/PTY/SSH-scars. Never `exec` or `cargo add`. |
| jcode | Anti-exemplar for providers (keep: named profiles, `grok login`). |
| Prime | Keep: generic `!` → shell. Drop: IPython kernel, bare-word env lookup. |
| Omarchy | DHH's daily Linux. Precedent for omakase: one chef, one machine, no committee desktop. |
| cordis / DSH (`~/src/ext/cordis`, `~/src/ext/deepseek-harness`) | Scar: service persists, fiber occupies a slot; inspect / mount / total unmount. Never `exec` or depend. |

Subagents are more smiths, each with their own anvil and hammer. No
special protocol beyond “another process.”

## Concepts

Kernel (`docs/kernel.md`). Use these words on the window seat:

| Word | What it is |
|---|---|
| **window** | App frame. Fiber on the tty. `prefix+q` unmounts it. |
| **panel** | Bounded content. Occupies one slot. Text, terminal, or rows. |
| **slot** | Named seat a fiber occupies. Does not store a service. |
| **service** | Persists on serve. A terminal is one. Destroying a viewer does not destroy it. |
| **fiber** | Occupies a slot; dispose reverts its effects. Window and panel are fibers. |
| **terminal** | What you sit in. A service. PTY is the adapter, not a UX word. |
| **rows** | A panel whose slots are stacked. A list of any panels. |
| **serve** | The context. Owns services. Unix socket. |
| **inspect** | Live services, fibers, slots. |
| **mount / unmount** | Put a fiber on a slot / dispose it completely. |
| **smith** | The binary you launch. `--experience window` is the growth seat. |
| **anvil** | The harness. Binary: `anvil` (CLI, serve, strike). |
| **hammer** | Stock CPython guest. Dies. We hang another. |
| **strike** | One `eval` of model-written Python. The only tool. |
| **ask** | Model writes Python; extract; strike. Not hung on the window yet. |

The older TUI still uses a larger vocabulary in `src/frame/` and
`src/tui/mod.rs`. Do not add new work there unless the window seat
cannot take the bruise. Do not invent column splits or a “list
control” — a list is a panel of row-slots.

## Layout

```
one Cargo.toml, two bins
  src/lib.rs           harness: spawn hammer, strike, respawn
  src/protocol.rs      newline-JSON types
  src/secret.rs        !command / $ENV / literal
  src/config.rs        named YAML providers
  src/oauth.rs         vendor login (grok)
  src/catalog.rs       /models + cache
  src/complete.rs      chat/completions smoke
  src/ask.rs           model → extract Python → strike
  src/frame/           sessions, workspaces, catalogs, layouts, transcript
  src/serve/           unix socket daemon + client + PTY host
  src/tui/window.rs    growth seat: window / panel / rows / terminal
  src/tui/experience.rs  smith | window launch switch
  src/tui/             older full TUI (rail, ask, tiles) — keep, do not grow first
  src/tui/theme.rs     named faces + ink packs (mocha, terminal)
  src/tui/keys.rs      prefix keymap (every action named)
  src/prof.rs          ns ring + Timing (prefill, TTFT, decode, tok/s, strike)
  src/bin/anvil.rs     CLI
  src/bin/smith.rs     TUI binary (`--experience window`)
hammer/hammer.py       guest
config.example.yaml    shape for ~/.config/anvil/config.yaml
docs/kernel.md         standalone ontology for the window seat
skills/dev/SKILL.md    how to run and test
```

## Commands

```bash
cargo test
cargo build --release --bins     # then PATH smith/anvil pick it up
anvil strike 'print(2+2)'
anvil providers
anvil login grok
anvil models --refresh
anvil ask -p nim 'how many files have synlinks ~/dotfiles/ (recursive)'
anvil session new                    # mints a short unused word
anvil session new audit
anvil workspace add fleet-os audit
anvil workspace add fleet-os bash --pty
anvil workspace add fleet-os clock --clock
anvil workspace add fleet-os audit --log
anvil workspace add fleet-os notes --edit
anvil edit write notes hello
anvil edit snap notes
anvil pty new bash --workspace fleet-os
anvil pty snap bash                  # live screen; warms $SHELL
anvil pty write bash echo anvil-pty-ok
anvil catalog add compute-saturation fleet-os
anvil serve --status
anvil serve --install            # user unit + linger; cold after reboot
smith --remote prince -p nim     # SSH; sessions stay on prince
anvil inspect
anvil prof                       # live ring: prefill/TTFT/tok/s/strike/frame
anvil session log audit
anvil mount clock
anvil unmount dyn-1
smith --experience window        # growth seat; starts serve; prefix+q detaches
smith -p nim                     # older full TUI
smith -s audit -p nim
```

`ANVIL_ROOT` default `$HOME/.anvil`. Sessions live in
`~/.anvil/sessions/<id>/` (legacy pickle at `~/.anvil/default` is
session `default`). Serve listens on `$XDG_RUNTIME_DIR/anvil.sock`
(`ANVIL_SOCK`). `ANVIL_STORE` is a raw store, no rail, no serve.
`ANVIL_HAMMER` overrides the guest. `ANVIL_CONFIG` default
`~/.config/anvil/config.yaml`.

PATH: `~/.local/bin/{smith,anvil}` are the same wrapper (`scripts/launch`).
It execs `target/release/<name>` from **stable** (primary clone) or **dev**
(a worktree). Not git. Default channel is stable.

```bash
anvil channel                 # show
anvil channel stable          # daily: ~/src/witt3rd/anvil/target/release
anvil channel dev ~/src/witt3rd/anvil.wt/feat--foo
cargo build --release --bins  # in that tree, then smith picks it up
```

## Providers

Secrets (Prime upstream: `!` is any shell command, not a Doppler hook):

| Form | Meaning |
|---|---|
| `sk-…` | literal |
| `$NAME` / `${NAME}` | environment |
| `!rest` | `sh -c rest`, trimmed stdout (e.g. `!doppler secrets get KEY -p proj -c cfg --plain`) |

`GET {base_url}/models` caches at `~/.cache/anvil/models/<name>.json`
for 24h. Completions are OpenAI-compatible `/v1/chat/completions`. Grok
oauth only supplies a token; we do not speak ACP.

## Git

House rules (`git` skill). This file only names what is true *here*.

- **Primary** `~/src/witt3rd/anvil` stays on `main`. Never check it out to
  a feature branch. Never commit from it. The founding commit on `main`
  was the last exception.
- **Work** is a linked worktree: `git wt-new feat/foo` →
  `~/src/witt3rd/anvil.wt/feat--foo/`. Do not `git worktree add` by hand.
  `git wt-rm feat/foo` after the branch is on `main`.
- **Mainline** is `main` (not `master`). `origin` =
  `git@github.com:witt3rd/anvil.git` (public). There is no `upstream` —
  this is our repo, not a fork.
- **Land** by merging to `main` (PR or merge from the worktree), then
  `git wt-rm`. Verify `origin/main` has not moved first. Do not make
  `feat/providers` a permanent second mainline; it is a branch like any
  other until it lands.
- **Daily binaries** are the launch wrapper + `anvil channel`. Stable is
  a release build of `main`. Dev is a release build of a worktree.

`gh` routes to `witt3rd` for this remote. Account switching is the
`git` skill, not this file.

## Caretaker

Leave the repo cleaner than you found it. This file is the charter.
If a change does not make the **window** better to sit with, it is
the wrong change. `docs/kernel.md` is the ontology. `skills/dev/SKILL.md`
is how-to. A bruise we keep goes here (rule) or in the skill
(procedure). Unmount must restore the tty (`Restore` in
`src/tui/window.rs`).
