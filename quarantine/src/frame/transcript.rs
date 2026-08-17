use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};

use serde::{Deserialize, Serialize};

use super::{FrameError, FrameRoot};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum TranscriptLine {
    User {
        text: String,
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
    },
    Answer {
        text: String,
    },
    Status {
        text: String,
    },
}

impl FrameRoot {
    pub fn transcript_path(&self, session: &str) -> Result<std::path::PathBuf, FrameError> {
        let sess = if self.session_exists(session) {
            self.session(session)?
        } else {
            return Err(FrameError::UnknownSession(session.into()));
        };
        Ok(sess.dir.join("transcript.jsonl"))
    }

    pub fn append_transcript(
        &self,
        session: &str,
        line: &TranscriptLine,
    ) -> Result<(), FrameError> {
        let path = self.transcript_path(session)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        let mut data = serde_json::to_vec(line).expect("transcript line is serializable");
        data.push(b'\n');
        file.write_all(&data)?;
        Ok(())
    }

    pub fn load_transcript(&self, session: &str) -> Result<Vec<TranscriptLine>, FrameError> {
        let path = self.transcript_path(session)?;
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path)?;
        let mut lines = Vec::new();
        for raw in BufReader::new(file).lines() {
            let raw = raw?;
            if raw.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(&raw) {
                Ok(line) => lines.push(line),
                Err(source) => {
                    return Err(FrameError::Json {
                        path: path.clone(),
                        source,
                    });
                }
            }
        }
        Ok(lines)
    }
}
