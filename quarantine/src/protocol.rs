use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrikeRequest {
    pub id: String,
    pub op: Op,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Strike,
    Ping,
    Reset,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrikeReply {
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub value: serde_json::Value,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub error: Option<String>,
}

impl StrikeRequest {
    pub fn strike(id: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            op: Op::Strike,
            code: Some(code.into()),
        }
    }

    pub fn ping(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            op: Op::Ping,
            code: None,
        }
    }

    pub fn reset(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            op: Op::Reset,
            code: None,
        }
    }

    pub fn shutdown(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            op: Op::Shutdown,
            code: None,
        }
    }
}
