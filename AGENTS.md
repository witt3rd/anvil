# anvil

**smith** is who you sit with. This repo is named for the block smith
stands at. One operator (Donald, on roger and the fleet). The model
writes Python. Work lives outside the prompt. Not a category — a daily
seat.

## Goal

You launch **smith**. You talk to smith. Bruises happen in smith.

Fixed tool menus (`read_file`, `bash`, `edit`, …) dump every result into
the context window. Compaction then deletes. smith inverts that: the
model does not pick from a menu. It writes Python. anvil **strikes** it
on the hammer. Intermediate data stays as variables. Only what the
guest **prints** or **returns** comes back into the transcript.

The LLM does not *use tools*. It *decides what to strike*. smith is the
person at the block. anvil is the block. The hammer hits.

We grow by **dogfooding** (live in smith) and **demand paging** (fault
in a capability the first time a day in smith trips over its absence,
then compact). We do not build a platform in advance of a bruise.

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
7. **Secrets stay out of the binary and out of logs.** Resolve
   `!doppler …` / `$ENV` at use. Never print the resolved value. Bare
   words are literals, not env names.
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

**Now:** one smith, in-process anvil, one hammer, named HTTP providers,
`ask` = complete → extract Python → strike. Daily binaries are release
builds of `feat/providers` on `PATH`.

**When use bruises us, in this order:**

1. In-process **tiles** — two smiths, or smith | pty (a real shell).
   Textbook: `~/src/ext/zellij` (screen, terminal pane, pty bus). Not a
   cargo dep.
2. **`anvil serve`** — hammer outlives the TUI; smith attaches on a
   unix socket.
3. **SSH attach** — `ssh host smith` / `ssh host anvil attach`. Recipe:
   herdr `src/remote/attach.rs` (stdio + socket). Not zellij's web
   remote. Not herdr as a host.

**Neighbors (stay neighbors):**

| Tree | Role |
|---|---|
| herdr (`~/src/ext/herdr`) | The desktop mux. smith may run *in* a herdr pane as a process. herdr must not own anvil. |
| zellij (`~/src/ext/zellij`) | Textbook for tiles/PTY/SSH-scars. Never `exec` or `cargo add`. |
| jcode | Anti-exemplar for providers (keep: named profiles, `grok login`). |
| Prime | Keep: `!doppler`. Drop: IPython kernel, bare-word env lookup. |

Subagents are more smiths, each with their own anvil and hammer. No
special protocol beyond “another process.”

## Concepts

| Word | What it is |
|---|---|
| **smith** | Who you talk to. The daily seat. TUI + the ask loop you see. Binary: `smith`. |
| **anvil** | The harness under smith. Does not move. Binary: `anvil` (CLI, serve, strike). |
| **hammer** | Stock CPython guest. Hits the work. Dies. We hang another. |
| **strike** | One `eval`. A blow, not a process. The only tool. |
| **store** | On-disk workspace (`~/.anvil/default/namespace.pkl`). Not “the bench.” |
| **ask** | Model writes Python; extract; strike; print stdout. Not `complete`. |
| **complete** | Raw HTTP chat. Will waffle. Smoke only. |
| **tile** | A pane we own: smith or pty. Not yet built. |

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
  src/tui/             smith TUI (blocks, worker, @ picker)
  src/bin/anvil.rs     CLI
  src/bin/smith.rs     TUI binary
hammer/hammer.py       guest
config.example.yaml    shape for ~/.config/anvil/config.yaml
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
smith -p nim                     # daily seat
```

`ANVIL_STORE` default `$HOME/.anvil/default`. `ANVIL_HAMMER` overrides
the guest. `ANVIL_CONFIG` default `~/.config/anvil/config.yaml`.

PATH launchers (this box):

```
~/.local/bin/smith → …/anvil.wt/feat--providers/target/release/smith
~/.local/bin/anvil → …/anvil.wt/feat--providers/target/release/anvil
```

Rebuild after a change: `cargo build --release --bins` in that worktree.

## Providers

Secrets:

| Form | Meaning |
|---|---|
| `sk-…` | literal |
| `$NAME` / `${NAME}` | environment |
| `!doppler secrets get KEY -p proj -c cfg --plain` | `sh -c`, trimmed stdout |

`GET {base_url}/models` caches at `~/.cache/anvil/models/<name>.json`
for 24h. Completions are OpenAI-compatible `/v1/chat/completions`. Grok
oauth only supplies a token; we do not speak ACP.

## Git

`origin` = `git@github.com:witt3rd/anvil.git` (public). Primary clone
`~/src/witt3rd/anvil` stays on `main`. Work in `anvil.wt/<branch>/` via
`git wt-new`. Daily driver tracks `feat/providers` until it lands.

## Caretaker

Leave the repo cleaner than you found it. This file is the charter.
If a change does not make **smith** better to sit with, it is the wrong
change.
`skills/dev/SKILL.md` is how-to. A bruise from daily use that we decide
to keep goes here (if it is a rule) or in the skill (if it is a
procedure).
