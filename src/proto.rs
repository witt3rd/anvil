//! The wire contract between the client and the daemon. Ops are JSON
//! objects, one per line (`docs/protocol.md`). A pane's grid follows
//! its reply as packed cells, not JSON. A new op needs a new kernel
//! word first.

use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::daemon::pane::Grid;
use crate::daemon::session::SessionView;

/// A packed pane view this large is not a terminal; it is a fault.
const MAX_GRID_BYTES: u32 = 4 * 1024 * 1024;

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// The names of the sessions the daemon owns.
    Enumerate { id: String },
    /// A new session with one window, or a new window in a session.
    Create {
        id: String,
        session: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<String>,
    },
    /// Put a client on a session.
    Attach { id: String, session: String },
    /// The session under its new name, or a window in that session.
    /// `note` is the markdown blob stored with that window.
    Rename {
        id: String,
        session: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// The session is gone; its windows and panes with it.
    Destroy { id: String, session: String },
    /// The session's windows, their panes, each pane's geometry, and
    /// the focused pane; or a pane's grid.
    Read {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<String>,
        /// Rows of PTY history above the live screen. Absent is the bottom.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scroll: Option<u16>,
        /// Last packed view the client holds. The daemon replies `same`
        /// and sends no cells when the pane has not changed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gen: Option<u64>,
    },
    /// A window is now two panes, tiled. `rows` stacks them
    /// (split down); the default is side by side (split right).
    Split {
        id: String,
        window: String,
        #[serde(default, skip_serializing_if = "is_false")]
        rows: bool,
    },
    /// Move the focus: a window becomes the current one, or a pane
    /// becomes the focused pane.
    Focus {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<String>,
    },
    /// The pane or window is gone; its processes end (SIGHUP).
    Close {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<String>,
    },
    /// The panes relaid out to the new tty; the processes told
    /// (`SIGWINCH`).
    Resize { id: String, cols: u16, rows: u16 },
    /// A process runs on the pane. PTY by default; `acp` holds stdio.
    Spawn {
        id: String,
        pane: String,
        program: String,
        #[serde(default)]
        acp: bool,
        /// HTTP door of the TUI (`http://127.0.0.1:port`). The daemon
        /// watches it for rail state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        watch: Option<String>,
        /// Catalog name when this process is an agent.
        /// Absent on a shell.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Directory to start the process in. Agents key sessions on it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    /// The data goes to the focused pane's process, or a named pane.
    /// `prompt` is a turn on an agent door (HTTP or ACP), not keys.
    Write {
        id: String,
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        prompt: bool,
    },
    /// The client donates its stdout. The next SCM_RIGHTS message is
    /// the tty the daemon paints.
    Tty { id: String },
    /// A key, mouse, or resize from the client's tty. The daemon is
    /// the keyboard: prefix stays here; anything else is a write.
    Input { id: String, event: Input },
}

impl Request {
    pub fn id(&self) -> &str {
        match self {
            Self::Enumerate { id }
            | Self::Create { id, .. }
            | Self::Attach { id, .. }
            | Self::Rename { id, .. }
            | Self::Destroy { id, .. }
            | Self::Read { id, .. }
            | Self::Split { id, .. }
            | Self::Focus { id, .. }
            | Self::Close { id, .. }
            | Self::Resize { id, .. }
            | Self::Spawn { id, .. }
            | Self::Write { id, .. }
            | Self::Tty { id }
            | Self::Input { id, .. } => id,
        }
    }
}

/// A key, mouse, or resize from the client's tty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Input {
    Key {
        code: String,
        #[serde(default)]
        ch: Option<char>,
        #[serde(default)]
        mods: u8,
    },
    Mouse {
        button: String,
        col: u16,
        row: u16,
        #[serde(default)]
        mods: u8,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Focus {
        gained: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reply {
    pub id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Byte count of a packed grid that follows this JSON line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<u32>,
    /// The pane has not changed since the client's `gen`. No cells follow.
    #[serde(default, skip_serializing_if = "is_false")]
    pub same: bool,
    /// The pane's view generation. The client sends it back on the next read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen: Option<u64>,
}

impl Reply {
    pub fn ok(id: &str, value: Value) -> Reply {
        Reply {
            id: id.into(),
            ok: true,
            value: Some(value),
            error: None,
            grid: None,
            same: false,
            gen: None,
        }
    }

    pub fn err(id: &str, error: impl Into<String>) -> Reply {
        Reply {
            id: id.into(),
            ok: false,
            value: None,
            error: Some(error.into()),
            grid: None,
            same: false,
            gen: None,
        }
    }

    pub fn same(id: &str, gen: u64) -> Reply {
        Reply {
            id: id.into(),
            ok: true,
            value: None,
            error: None,
            grid: None,
            same: true,
            gen: Some(gen),
        }
    }

    /// Write this reply. A pane grid is packed bytes after the JSON
    /// line, not a JSON value.
    pub fn write_to<W: Write>(mut self, writer: &mut W) -> io::Result<()> {
        let payload = match self.value.take() {
            Some(Value::Grid(grid)) => {
                let bytes = grid.pack();
                self.grid = Some(bytes.len() as u32);
                if self.gen.is_none() {
                    self.gen = Some(grid.gen);
                }
                Some(bytes)
            }
            other => {
                self.value = other;
                None
            }
        };
        let mut line = serde_json::to_string(&self).map_err(io::Error::other)?;
        line.push('\n');
        writer.write_all(line.as_bytes())?;
        if let Some(bytes) = payload {
            writer.write_all(&bytes)?;
        }
        writer.flush()
    }

    /// Read one reply. If `grid` is set, the next that many bytes are
    /// a packed pane view.
    pub fn read_from<R: BufRead>(reader: &mut R) -> io::Result<Reply> {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the daemon closed the socket",
            ));
        }
        let mut reply: Reply = serde_json::from_str(&line).map_err(io::Error::other)?;
        if let Some(len) = reply.grid {
            if len > MAX_GRID_BYTES {
                return Err(io::Error::other("grid larger than a pane"));
            }
            let mut buf = vec![0u8; len as usize];
            reader.read_exact(&mut buf)?;
            let mut grid = Grid::unpack(&buf)?;
            if let Some(g) = reply.gen {
                grid.gen = g;
            }
            reply.value = Some(Value::Grid(grid));
            reply.grid = None;
        }
        Ok(reply)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Value {
    /// enumerate: the names of the sessions the daemon owns, and
    /// the git of this ELF (`build`). Absent on an older daemon.
    Sessions {
        sessions: Vec<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        build: String,
    },
    /// read a session: its windows, their panes, each pane's geometry,
    /// and the focused pane.
    View(SessionView),
    /// read a pane: the pane's grid — its cols, rows, and cells.
    Grid(Grid),
    /// Everything else.
    Empty {},
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_the_wire_shapes() {
        let cases = [
            (r#"{"op":"enumerate","id":"a"}"#, "a"),
            (r#"{"op":"create","id":"b","session":"work"}"#, "b"),
            (
                r#"{"op":"create","id":"c","session":"work","window":"2"}"#,
                "c",
            ),
            (r#"{"op":"attach","id":"d","session":"work"}"#, "d"),
            (
                r#"{"op":"rename","id":"e","session":"work","name":"deep"}"#,
                "e",
            ),
            (r#"{"op":"destroy","id":"f","session":"work"}"#, "f"),
            (r#"{"op":"read","id":"g","session":"work"}"#, "g"),
            (r#"{"op":"read","id":"h","pane":"1"}"#, "h"),
            (r#"{"op":"split","id":"i","window":"1"}"#, "i"),
            (r#"{"op":"focus","id":"j","window":"1"}"#, "j"),
            (r#"{"op":"focus","id":"k","pane":"1"}"#, "k"),
            (r#"{"op":"resize","id":"j","cols":100,"rows":40}"#, "j"),
            (r#"{"op":"spawn","id":"k","pane":"1","program":"sh"}"#, "k"),
            (r#"{"op":"write","id":"l","data":"echo hi\n"}"#, "l"),
        ];
        for (wire, id) in cases {
            let req: Request = serde_json::from_str(wire).unwrap_or_else(|e| panic!("{wire}: {e}"));
            assert_eq!(req.id(), id);
            let back = serde_json::to_string(&req).unwrap();
            let again: Request = serde_json::from_str(&back).unwrap();
            assert_eq!(req, again);
        }
    }

    #[test]
    fn reply_round_trips_ok_and_err() {
        let ok = Reply::ok("a", Value::Empty {});
        let wire = serde_json::to_string(&ok).unwrap();
        assert_eq!(wire, r#"{"id":"a","ok":true,"value":{}}"#);
        let back: Reply = serde_json::from_str(&wire).unwrap();
        assert_eq!(ok, back);

        let err = Reply::err("a", "no such session");
        let wire = serde_json::to_string(&err).unwrap();
        assert_eq!(wire, r#"{"id":"a","ok":false,"error":"no such session"}"#);
        let back: Reply = serde_json::from_str(&wire).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn a_pane_grid_is_packed_bytes_not_json() {
        let grid = Grid {
            cols: 4,
            rows: 1,
            cursor_col: 0,
            cursor_row: 0,
            lines: vec!["ab  ".into()],
            runs: vec![vec![crate::daemon::pane::Run {
                text: "ab  ".into(),
                fg: None,
                fg_rgb: None,
                bg: None,
                bg_rgb: None,
                bold: false,
                italic: false,
                underline: false,
                inverse: false,
            }]],
            alive: true,
            acp: false,
            mouse: false,
            kitty: 0,
            modify: false,
            alternate: false,
            scroll: 0,
            gen: 0,
        };
        let mut buf = Vec::new();
        Reply::ok("h", Value::Grid(grid.clone()))
            .write_to(&mut buf)
            .unwrap();
        let json_end = buf.iter().position(|&b| b == b'\n').unwrap();
        let header = std::str::from_utf8(&buf[..=json_end]).unwrap();
        assert!(
            !header.contains("ab"),
            "cells must not be in the JSON line: {header}"
        );
        assert!(header.contains("\"grid\":"), "{header}");
        let back = Reply::read_from(&mut buf.as_slice()).unwrap();
        match back.value {
            Some(Value::Grid(g)) => assert_eq!(g, grid),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unchanged_pane_is_same_not_cells() {
        let mut buf = Vec::new();
        Reply::same("h", 7).write_to(&mut buf).unwrap();
        let header = std::str::from_utf8(&buf).unwrap();
        assert!(header.contains("\"same\":true"), "{header}");
        assert!(!header.contains("\"grid\""), "{header}");
        let back = Reply::read_from(&mut buf.as_slice()).unwrap();
        assert!(back.same);
        assert_eq!(back.gen, Some(7));
        assert!(back.value.is_none());
    }

    #[test]
    fn enumerate_build_defaults_on_old_wire() {
        let v: Value = serde_json::from_str(r#"{"sessions":["work"]}"#).unwrap();
        let Value::Sessions { sessions, build } = v else {
            panic!("{v:?}");
        };
        assert_eq!(sessions, vec!["work".to_string()]);
        assert!(build.is_empty());
    }
}
