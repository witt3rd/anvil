# Kernel

A small seat for living with processes and a language model, built from
a handful of words. This document is the whole ontology for that seat.
It does not assume you have seen the rest of this repo.

You launch a **window**. Inside it, **panels** occupy **slots**. A panel
can show a **terminal**, a line of text, or more panels stacked as rows
(a list of anything). A long-lived process called **serve** owns the
things that should survive you closing the window. A second process, the
**hammer**, is stock CPython. When a model wants the machine to do
something, it writes Python; serve **strikes** that code on the hammer.
Work stays in the guest as variables. Only what the guest prints or
returns comes back.

That is the kernel. Everything else is a panel we have not hung yet.

## Two lifetimes

Most of the design is one distinction, taken from Cordis and kept
verbatim.

A **service** is provided and persists. Destroying a viewer does not
destroy it. A shell that keeps running after you detach is a service.
So is a Python namespace the hammer is holding.

A **fiber** occupies something for a while and then goes away. When you
dispose a fiber, every effect it had is undone: it stops drawing, it
stops sending keys, it gives the tty back. The window you are looking
at is a fiber. A panel inside it is a fiber.

A **slot** is a named seat a fiber can sit in. Without a slot there is
nowhere to hang a fiber. Without a complete dispose, a fiber leaks
(the window stays on the alternate screen; the login shell looks dead).

A **context** is who owns the services. Here that is **serve**.

Three verbs:

| Verb | What it does |
|---|---|
| **inspect** | What services, fibers, and slots exist right now |
| **mount** | Put a fiber on a slot |
| **unmount** | Dispose that fiber completely |

A request (`anvil inspect`, `anvil serve --status`) borrows the tty and
gives it back. A fiber (`smith --experience window`) occupies the tty
until you unmount it.

## What you see

Two everyday words.

A **window** is the app frame. One live surface on one tty. Focus lives
here. Launching the window mounts a fiber on the tty. `prefix+q`
(default `ctrl+b` then `q`) unmounts it and restores the login shell.

A **panel** is a bounded region of content. It occupies one slot. A
panel is one of:

- **text** — one row
- **terminal** — injects a terminal service (a login shell)
- **rows** — more slots, stacked. Each slot holds a panel. That is a
  list. The children need not be text. They can be terminals, or more
  lists.

There is no separate “list control.” A list is a panel whose slots are
rows.

```
tty                          slot
 └── window                  fiber
       └── panel (rows)
             slot → panel (rows)          list of services
                      slot → panel (text)
                      slot → panel (text)
             slot → panel (terminal)      leftover height
                                           ──injects──▶ terminal service
```

**Containment** is a tree. The window contains panels. Destroy the
window, those panels die.

**Injection** is a binding. A terminal panel injects a terminal
*service* that serve already has. Many panels, over time, may inject
the same service. Unmounting the panel (or the whole window) stops the
drawing and the typing. The service stays.

A slot does not store a shell. Persistence is the service, not the
seat.

The only spatial rule that has earned its keep: **rows**. A text panel
is one row tall. A terminal panel takes the leftover height. Column
splits, tabs, and chrome rails are not in this kernel.

## The guest

The model does not pick from a tool menu. Tool menus dump every result
into the prompt; compaction then deletes.

Instead the model writes Python. **anvil** is the block. The **hammer**
is stock CPython (one JSON line in, one JSON line out — no IPython, no
magics). A **strike** is: run that code in the guest. Intermediate
values stay as variables in the namespace. Only stdout, the return
value, and errors come back.

The guest will die. That is why it is not also the window. Serve stays
up; the hammer is replaceable. The namespace lives on disk next to
serve.

This path is built (`anvil strike`, `anvil ask`, the default `smith`
seat). The window described above does not open it yet. It is in the
pocket: another kind of service a panel will inject when we need it.

## Where the code is

Launch either seat from the same binary:

```bash
smith                         # the older, full seat
smith --experience window     # this kernel
```

`prefix+q` leaves the window. Ctrl+C is not sent to the inner shell
here. Serve keeps the terminal; the next `smith --experience window`
attaches to the same one.

| Idea | Code |
|---|---|
| Window fiber on the tty | `src/tui/window.rs` (`run`, `Restore`) |
| Which seat to launch | `src/tui/experience.rs`, `src/bin/smith.rs` (`--experience`) |
| Panel = text / terminal / rows | `src/tui/window.rs` (`Panel`) |
| Rows get leftover height | `src/tui/window.rs` (`place_rows`) |
| Terminal service on serve | `src/serve/pty.rs` (`PtyHost`); window talks via `src/serve/client.rs` |
| Attach, do not own | `window.rs` `Shell::attach` — no shutdown on detach |
| Inspect → list of services | `src/serve/inspect.rs`; each service becomes a text panel in a row slot |
| Keys to the terminal | `src/tui/term.rs` `key_bytes` |
| Context process | `src/serve/mod.rs`, `anvil serve` |
| Strike / hammer | `src/lib.rs`, `hammer/hammer.py`, `anvil strike` |
| Model writes Python | `src/ask.rs`, `anvil ask` |
| Other machine | `src/remote.rs` — `smith --remote HOST` is SSH; processes stay on HOST |

Serve listens on `$XDG_RUNTIME_DIR/anvil.sock`. Closing the window does
not stop serve. `anvil serve --stop` does. `anvil serve --install`
writes a user systemd unit so serve returns after login.

Remote is SSH, not a custom wire. `smith --remote prince` / `anvil
--remote prince inspect` is `ssh prince -- smith|anvil …`. That host
needs the binary on PATH (and, for boot, `anvil serve --install`).

## Where this is going

Demand-page. Hang a panel when a day in the seat requires it. Do not
invent a platform first.

The next things this ontology already allows, without new kinds:

- A row slot holding a terminal instead of text (lists of shells)
- A row slot holding another row-list (groups)
- A panel that injects the hammer — the model writes Python in place
- Someone else’s agent as a service, shown by a panel, same as a
  terminal
- Another window on another tty, or on another machine over SSH,
  injecting the same services

The kernel is small on purpose. Window, panel, slot, service, fiber.
A tty, a shell that outlives the window, a guest that is just CPython,
a model that writes code instead of picking tools. That is enough to
grow a mux, an agent, or both, without changing the words.
