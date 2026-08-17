//! The wire contract between the client and the daemon: one JSON
//! object per line over the unix socket. Ops are the documented verbs
//! only (`docs/protocol.md`). A new op needs a new kernel word first.

use serde::{Deserialize, Serialize};

use crate::daemon::pane::Grid;
use crate::daemon::session::SessionView;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// The names of the sessions the daemon owns.
    Enumerate {
        id: String,
    },
    /// A new session with one window, or a new window in a session.
    Create {
        id: String,
        session: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<String>,
    },
    /// Put a client on a session.
    Attach {
        id: String,
        session: String,
    },
    /// The session under its new name.
    Rename {
        id: String,
        session: String,
        name: String,
    },
    /// The session is gone; its windows and panes with it.
    Destroy {
        id: String,
        session: String,
    },
    /// The session's windows, their panes, each pane's geometry, and
    /// the focused pane; or a pane's grid.
    Read {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<String>,
    },
    /// A window is now two panes, tiled.
    Split {
        id: String,
        window: String,
    },
    /// The panes relaid out to the new tty; the processes told
    /// (`SIGWINCH`).
    Resize {
        id: String,
        cols: u16,
        rows: u16,
    },
    /// A process runs on the pane's slave PTY; the daemon holds the
    /// master.
    Spawn {
        id: String,
        pane: String,
        program: String,
    },
    /// The data goes to the focused pane's process.
    Write {
        id: String,
        data: String,
    },
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
            | Self::Resize { id, .. }
            | Self::Spawn { id, .. }
            | Self::Write { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reply {
    pub id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    pub fn ok(id: &str, value: Value) -> Reply {
        Reply {
            id: id.into(),
            ok: true,
            value: Some(value),
            error: None,
        }
    }

    pub fn err(id: &str, error: impl Into<String>) -> Reply {
        Reply {
            id: id.into(),
            ok: false,
            value: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Value {
    /// enumerate: the names of the sessions the daemon owns.
    Sessions { sessions: Vec<String> },
    /// read a session: its windows, their panes, each pane's geometry,
    /// and the focused pane.
    View(SessionView),
    /// read a pane: the pane's grid — its cols, rows, and cells.
    Grid(Grid),
    /// Everything else.
    Empty {},
}#[cfg(test)]
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
            (r#"{"op":"rename","id":"e","session":"work","name":"deep"}"#, "e"),
            (r#"{"op":"destroy","id":"f","session":"work"}"#, "f"),
            (r#"{"op":"read","id":"g","session":"work"}"#, "g"),
            (r#"{"op":"read","id":"h","pane":"1"}"#, "h"),
            (r#"{"op":"split","id":"i","window":"1"}"#, "i"),
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
}
