use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{FrameError, FrameRoot};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMeta {
    pub id: String,
    pub created: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    pub id: String,
    pub dir: PathBuf,
    pub meta: SessionMeta,
}

impl FrameRoot {
    pub fn session_dir(&self, id: &str) -> PathBuf {
        if id == "default" {
            let named = self.root.join("sessions").join("default");
            if named.join("meta.json").is_file() || named.join("namespace.pkl").is_file() {
                return named;
            }
            let legacy = self.root.join("default");
            if legacy.join("namespace.pkl").is_file() || legacy.join("meta.json").is_file() {
                return legacy;
            }
        }
        self.root.join("sessions").join(id)
    }

    pub fn session_exists(&self, id: &str) -> bool {
        let dir = self.session_dir(id);
        dir.join("meta.json").is_file() || dir.join("namespace.pkl").is_file()
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionRef>, FrameError> {
        let mut out = Vec::new();
        let dir = self.root.join("sessions");
        if dir.is_dir() {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if self.session_exists(&name) {
                    out.push(self.session(&name)?);
                }
            }
        }
        if self.session_exists("default") && !out.iter().any(|s| s.id == "default") {
            out.push(self.session("default")?);
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    pub fn session(&self, id: &str) -> Result<SessionRef, FrameError> {
        let id = Self::parse_name(id)?;
        if !self.session_exists(&id) {
            return Err(FrameError::UnknownSession(id));
        }
        let dir = self.session_dir(&id);
        let meta_path = dir.join("meta.json");
        let meta = if meta_path.is_file() {
            self.read_json(&meta_path)?
        } else {
            SessionMeta {
                id: id.clone(),
                created: now_secs(),
                cwd: None,
                provider: None,
                model: None,
            }
        };
        Ok(SessionRef { id, dir, meta })
    }

    pub fn create_session(&self, id: &str) -> Result<SessionRef, FrameError> {
        let id = Self::parse_name(id)?;
        if self.session_exists(&id) {
            return Err(FrameError::SessionExists(id));
        }
        let dir = self.root.join("sessions").join(&id);
        fs::create_dir_all(&dir)?;
        let meta = SessionMeta {
            id: id.clone(),
            created: now_secs(),
            cwd: None,
            provider: None,
            model: None,
        };
        self.write_json(&dir.join("meta.json"), &meta)?;
        Ok(SessionRef { id, dir, meta })
    }

    pub fn save_session_meta(&self, meta: &SessionMeta) -> Result<(), FrameError> {
        let dir = self.session_dir(&meta.id);
        fs::create_dir_all(&dir)?;
        self.write_json(&dir.join("meta.json"), meta)
    }

    pub fn delete_session(&self, id: &str) -> Result<(), FrameError> {
        let sess = self.session(id)?;
        fs::remove_dir_all(&sess.dir)?;
        Ok(())
    }

    pub fn rename_session(&self, old: &str, new: &str) -> Result<String, FrameError> {
        let old = Self::parse_name(old)?;
        let new = Self::parse_name(new)?;
        if old == new {
            return Ok(new);
        }
        if self.session_exists(&new) {
            return Err(FrameError::SessionExists(new));
        }
        let sess = self.session(&old)?;
        let dest = self.root.join("sessions").join(&new);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&sess.dir, &dest)?;
        let mut meta = sess.meta;
        meta.id = new.clone();
        self.write_json(&dest.join("meta.json"), &meta)?;
        for mut ws in self.list_workspaces()? {
            let mut dirty = false;
            for m in &mut ws.members {
                if m.session_id() == Some(old.as_str()) {
                    m.set_id(new.clone());
                    dirty = true;
                }
            }
            if dirty {
                self.save_workspace(&ws)?;
            }
        }
        Ok(new)
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
