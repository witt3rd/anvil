use std::fs;

use serde::{Deserialize, Serialize};

use super::{FrameError, FrameRoot};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemberRef {
    Session { id: String },
    Pty { id: String },
}

impl MemberRef {
    pub fn session(id: impl Into<String>) -> Self {
        Self::Session { id: id.into() }
    }

    pub fn pty(id: impl Into<String>) -> Self {
        Self::Pty { id: id.into() }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Session { id } | Self::Pty { id } => id,
        }
    }

    pub fn is_pty(&self) -> bool {
        matches!(self, Self::Pty { .. })
    }

    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Session { id } => Some(id),
            Self::Pty { .. } => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Session { id } => id.clone(),
            Self::Pty { id } => format!("{id} · pty"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub name: String,
    #[serde(default)]
    pub members: Vec<MemberRef>,
}

impl Workspace {
    pub fn add_member(&mut self, member: MemberRef) {
        if !self.members.contains(&member) {
            self.members.push(member);
        }
    }

    pub fn remove_member(&mut self, member: &MemberRef) {
        self.members.retain(|m| m != member);
    }
}

impl FrameRoot {
    fn workspace_path(&self, name: &str) -> std::path::PathBuf {
        self.root.join("workspaces").join(format!("{name}.json"))
    }

    pub fn workspace_exists(&self, name: &str) -> bool {
        FrameRoot::parse_name(name)
            .map(|n| self.workspace_path(&n).is_file())
            .unwrap_or(false)
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, FrameError> {
        let mut out = Vec::new();
        let dir = self.root.join("workspaces");
        if !dir.is_dir() {
            return Ok(out);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            out.push(self.read_json(&path)?);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn workspace(&self, name: &str) -> Result<Workspace, FrameError> {
        let name = Self::parse_name(name)?;
        let path = self.workspace_path(&name);
        if !path.is_file() {
            return Err(FrameError::UnknownWorkspace(name));
        }
        self.read_json(&path)
    }

    pub fn create_workspace(&self, name: &str) -> Result<Workspace, FrameError> {
        let name = Self::parse_name(name)?;
        if self.workspace_exists(&name) {
            return Err(FrameError::WorkspaceExists(name));
        }
        let ws = Workspace {
            name,
            members: Vec::new(),
        };
        self.save_workspace(&ws)?;
        Ok(ws)
    }

    pub fn save_workspace(&self, ws: &Workspace) -> Result<(), FrameError> {
        let name = Self::parse_name(&ws.name)?;
        self.write_json(&self.workspace_path(&name), ws)
    }

    pub fn delete_workspace(&self, name: &str) -> Result<(), FrameError> {
        let name = Self::parse_name(name)?;
        let path = self.workspace_path(&name);
        if !path.is_file() {
            return Err(FrameError::UnknownWorkspace(name));
        }
        fs::remove_file(path)?;
        Ok(())
    }
}
