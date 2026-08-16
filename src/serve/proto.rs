use serde::{Deserialize, Serialize};

use crate::StrikeReply;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Req {
    Ping {
        id: String,
    },
    Strike {
        id: String,
        session: String,
        code: String,
    },
    Reset {
        id: String,
        session: String,
    },
    Ask {
        id: String,
        session: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    Shutdown {
        id: String,
    },
    Inspect {
        id: String,
    },
    /// Occupy the stage with this session. Does not warm a hammer.
    Expose {
        id: String,
        session: String,
    },
    Mount {
        id: String,
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slot: Option<String>,
    },
    Unmount {
        id: String,
        mount_id: String,
    },
    PtyOpen {
        id: String,
        name: String,
        cols: u16,
        rows: u16,
    },
    PtyWrite {
        id: String,
        name: String,
        data: String,
    },
    PtyResize {
        id: String,
        name: String,
        cols: u16,
        rows: u16,
    },
    PtySnap {
        id: String,
        name: String,
    },
}

impl Req {
    pub fn id(&self) -> &str {
        match self {
            Self::Ping { id }
            | Self::Strike { id, .. }
            | Self::Reset { id, .. }
            | Self::Ask { id, .. }
            | Self::Shutdown { id }
            | Self::Inspect { id }
            | Self::Expose { id, .. }
            | Self::Mount { id, .. }
            | Self::Unmount { id, .. }
            | Self::PtyOpen { id, .. }
            | Self::PtyWrite { id, .. }
            | Self::PtyResize { id, .. }
            | Self::PtySnap { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Msg {
    Pong {
        id: String,
    },
    Status {
        id: String,
        session: String,
        text: String,
    },
    Draft {
        id: String,
        session: String,
        text: String,
    },
    Strike {
        id: String,
        session: String,
        code: String,
        stdout: String,
        stderr: String,
        error: Option<String>,
        ok: bool,
    },
    Answer {
        id: String,
        session: String,
        text: String,
    },
    Reply {
        id: String,
        reply: StrikeReply,
    },
    Error {
        id: String,
        text: String,
    },
    Bye {
        id: String,
    },
    Inspect {
        id: String,
        report: crate::serve::Report,
    },
    Mounted {
        id: String,
        mount_id: String,
        mount_kind: String,
        slot: String,
    },
    Unmounted {
        id: String,
        mount_id: String,
    },
    PtyScreen {
        id: String,
        name: String,
        cols: u16,
        rows: u16,
        cursor_col: u16,
        cursor_row: u16,
        lines: Vec<String>,
        alive: bool,
    },
}

impl Msg {
    pub fn id(&self) -> &str {
        match self {
            Self::Pong { id }
            | Self::Status { id, .. }
            | Self::Draft { id, .. }
            | Self::Strike { id, .. }
            | Self::Answer { id, .. }
            | Self::Reply { id, .. }
            | Self::Error { id, .. }
            | Self::Bye { id }
            | Self::Inspect { id, .. }
            | Self::Mounted { id, .. }
            | Self::Unmounted { id, .. }
            | Self::PtyScreen { id, .. } => id,
        }
    }
}
