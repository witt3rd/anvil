use std::fs;

use serde::{Deserialize, Serialize};

use super::{FrameError, FrameRoot};

/// Saved arrangement of a catalog: what's front, and pane weights.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Layout {
    pub name: String,
    pub catalog: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front_workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front_session: Option<String>,
    /// Relative heights of stage members (workspace order). Empty = equal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weights: Vec<u16>,
}

impl Layout {
    pub fn for_catalog(name: impl Into<String>, catalog: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            catalog: catalog.into(),
            front_workspace: Some("default".into()),
            front_session: Some("default".into()),
            weights: Vec::new(),
        }
    }
}

impl FrameRoot {
    fn layout_path(&self, name: &str) -> std::path::PathBuf {
        self.root.join("layouts").join(format!("{name}.json"))
    }

    pub fn layout_exists(&self, name: &str) -> bool {
        FrameRoot::parse_name(name)
            .map(|n| self.layout_path(&n).is_file())
            .unwrap_or(false)
    }

    pub fn layout(&self, name: &str) -> Result<Layout, FrameError> {
        let name = Self::parse_name(name)?;
        let path = self.layout_path(&name);
        if !path.is_file() {
            return Err(FrameError::UnknownLayout(name));
        }
        self.read_json(&path)
    }

    pub fn save_layout(&self, layout: &Layout) -> Result<(), FrameError> {
        let name = Self::parse_name(&layout.name)?;
        self.write_json(&self.layout_path(&name), layout)
    }

    pub fn delete_layout(&self, name: &str) -> Result<(), FrameError> {
        let name = Self::parse_name(name)?;
        let path = self.layout_path(&name);
        if !path.is_file() {
            return Err(FrameError::UnknownLayout(name));
        }
        fs::remove_file(path)?;
        Ok(())
    }
}
