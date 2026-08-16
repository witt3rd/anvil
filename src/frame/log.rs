//! Append-only event log for one member. Cards and ask project this.
//! Model-visible means logged.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::transcript::TranscriptLine;
use super::{FrameError, FrameRoot};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub seq: u64,
    pub ts: u64,
    #[serde(flatten)]
    pub body: EventBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventBody {
    User {
        text: String,
    },
    Ask {
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    Thinking {
        text: String,
    },
    Strike {
        code: String,
        stdout: String,
        stderr: String,
        error: Option<String>,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ms: Option<u64>,
    },
    Answer {
        text: String,
    },
    Status {
        text: String,
    },
    Fiber {
        state: String,
    },
}

impl EventBody {
    /// Reaches a model request. Thinking/status/fiber do not.
    pub fn model_visible(&self) -> bool {
        matches!(
            self,
            Self::User { .. } | Self::Ask { .. } | Self::Strike { .. } | Self::Answer { .. }
        )
    }
}

impl FrameRoot {
    pub fn events_path(&self, session: &str) -> Result<std::path::PathBuf, FrameError> {
        let sess = if self.session_exists(session) {
            self.session(session)?
        } else {
            return Err(FrameError::UnknownSession(session.into()));
        };
        Ok(sess.dir.join("events.jsonl"))
    }

    pub fn append_event(&self, session: &str, body: EventBody) -> Result<Event, FrameError> {
        self.ensure_migrated(session)?;
        let seq = self
            .load_events(session)?
            .last()
            .map(|e| e.seq + 1)
            .unwrap_or(0);
        let event = Event {
            seq,
            ts: now_ms(),
            body,
        };
        let path = self.events_path(session)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        let mut data = serde_json::to_vec(&event).expect("event is serializable");
        data.push(b'\n');
        file.write_all(&data)?;
        Ok(event)
    }

    pub fn load_events(&self, session: &str) -> Result<Vec<Event>, FrameError> {
        self.ensure_migrated(session)?;
        let path = self.events_path(session)?;
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path)?;
        let mut events = Vec::new();
        for raw in BufReader::new(file).lines() {
            let raw = raw?;
            if raw.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(&raw) {
                Ok(event) => events.push(event),
                Err(source) => {
                    return Err(FrameError::Json {
                        path: path.clone(),
                        source,
                    });
                }
            }
        }
        Ok(events)
    }

    fn ensure_migrated(&self, session: &str) -> Result<(), FrameError> {
        let events = match self.events_path(session) {
            Ok(p) => p,
            Err(_) => return Ok(()),
        };
        if events.is_file() {
            return Ok(());
        }
        let old = match self.transcript_path(session) {
            Ok(p) => p,
            Err(_) => return Ok(()),
        };
        if !old.is_file() {
            return Ok(());
        }
        let lines = self.load_transcript(session)?;
        if lines.is_empty() {
            return Ok(());
        }
        if let Some(parent) = events.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&events)?;
        for (seq, line) in lines.into_iter().enumerate() {
            let event = Event {
                seq: seq as u64,
                ts: 0,
                body: body_from_transcript(line),
            };
            let mut data = serde_json::to_vec(&event).expect("event is serializable");
            data.push(b'\n');
            file.write_all(&data)?;
        }
        Ok(())
    }
}

fn body_from_transcript(line: TranscriptLine) -> EventBody {
    match line {
        TranscriptLine::User { text } => EventBody::User { text },
        TranscriptLine::Thinking { text } => EventBody::Thinking { text },
        TranscriptLine::Strike {
            code,
            stdout,
            stderr,
            error,
            ok,
        } => EventBody::Strike {
            code,
            stdout,
            stderr,
            error,
            ok,
            ms: None,
        },
        TranscriptLine::Answer { text } => EventBody::Answer { text },
        TranscriptLine::Status { text } => EventBody::Status { text },
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameRoot;
    use tempfile::TempDir;

    #[test]
    fn append_assigns_seq_and_round_trips() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        root.create_session("fox").unwrap();
        root.append_event("fox", EventBody::User { text: "hi".into() })
            .unwrap();
        let strike = root
            .append_event(
                "fox",
                EventBody::Strike {
                    code: "1+1".into(),
                    stdout: "2\n".into(),
                    stderr: String::new(),
                    error: None,
                    ok: true,
                    ms: Some(3),
                },
            )
            .unwrap();
        assert_eq!(strike.seq, 1);
        let events = root.load_events("fox").unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[1].body.model_visible());
        assert!(!EventBody::Thinking { text: "hmm".into() }.model_visible());
    }

    #[test]
    fn migrates_legacy_transcript() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        root.create_session("fox").unwrap();
        root.append_transcript("fox", &TranscriptLine::User { text: "old".into() })
            .unwrap();
        let events = root.load_events("fox").unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].body {
            EventBody::User { text } => assert_eq!(text, "old"),
            other => panic!("{other:?}"),
        }
        assert!(root.events_path("fox").unwrap().is_file());
    }
}
