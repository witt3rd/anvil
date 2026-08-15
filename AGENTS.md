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

We grow by **radical dogfooding** (live in smith for hours; a papercut
is felt, then fixed) and **demand paging** (fault in a capability the
first time a day in smith trips over its absence, then compact). We do
not build a platform in advance of a bruise.

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

**Now:** one smith, in-process anvil, one hammer, named HTTP providers,
`ask` = complete → extract Python → strike. Daily binaries are release
builds of `feat/providers` on `PATH`.

**When use bruises us, in this order** (session frame first — that is
the current bruise; in-process tiles wait):

1. **Named sessions on disk** — persist store + transcript per
   session. `skills/dev/references/session-frame.md`.
2. **`anvil serve` + smith attach** — daemon owns hammers; smith
   detaches; work continues.
3. **Workspace + catalog + layout + casing** — benches and intents
   (conceptual); layouts and casings (UI). Inputs are just
   requests on the session.
4. **Reboot, then SSH** — as in that design.
5. In-process **tiles** (smith | pty) as workspace members — later.
   Textbook: Zellij.

**Neighbors (stay neighbors):**

| Tree | Role |
|---|---|
| herdr (`~/src/ext/herdr`) | The desktop mux. smith may run *in* a herdr pane as a process. herdr must not own anvil. |
| zellij (`~/src/ext/zellij`) | Textbook for tiles/PTY/SSH-scars. Never `exec` or `cargo add`. |
| jcode | Anti-exemplar for providers (keep: named profiles, `grok login`). |
| Prime | Keep: generic `!` → shell. Drop: IPython kernel, bare-word env lookup. |
| Omarchy | DHH's daily Linux. Precedent for omakase: one chef, one machine, no committee desktop. |

Subagents are more smiths, each with their own anvil and hammer. No
special protocol beyond “another process.”

## Concepts

| Word | What it is |
|---|---|
| **smith** | Who you talk to when the member is an anvil session: that pane. Binary `smith` launches a casing. |
| **anvil** | The harness under smith. Does not move. Binary: `anvil` (CLI, serve, strike). |
| **hammer** | Stock CPython guest. Hits the work. Dies. We hang another. |
| **strike** | One `eval`. A blow, not a process. The only tool. |
| **store** | On-disk persist for one session (`~/.anvil/sessions/<id>/namespace.pkl`). Not a workspace. |
| **session** | Conceptual. Anvil-specific: one coherent agentic process. A kind of member. |
| **member** | Conceptual. A machine process: a session, a bash, a web client, … |
| **workspace** | Conceptual. A collection of members. Everyday: the **bench**. Destroying it does not destroy members. |
| **catalog** | Conceptual. A collection of workspaces. A named intent. Destroying it does not destroy workspaces. |
| **pane** | UI. Exposes a member. Destroying it does not destroy the member. |
| **sash** | UI. Tabs or a list. Destroying it destroys its panes. A pane lives in one sash. |
| **window** | UI. A column of the casing. Destroying it destroys its sashes. No conceptual twin. |
| **layout** | UI, on disk. A saved arrangement of a catalog. |
| **casing** | UI, live. What `smith` launches. Destroys its windows. Many casings can load one layout. |
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
If a change does not make **smith** better to sit with, it is the wrong
change.
`skills/dev/SKILL.md` is how-to. A bruise from daily use that we decide
to keep goes here (if it is a rule) or in the skill (if it is a
procedure).
