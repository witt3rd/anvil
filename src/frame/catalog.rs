use std::fs;

use serde::{Deserialize, Serialize};

use super::{FrameError, FrameRoot};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Catalog {
    pub name: String,
    #[serde(default)]
    pub workspaces: Vec<String>,
}

impl Catalog {
    pub fn add_workspace(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !self.workspaces.iter().any(|w| w == &name) {
            self.workspaces.push(name);
        }
    }

    pub fn remove_workspace(&mut self, name: &str) {
        self.workspaces.retain(|w| w != name);
    }
}

impl FrameRoot {
    fn catalog_path(&self, name: &str) -> std::path::PathBuf {
        self.root.join("catalogs").join(format!("{name}.json"))
    }

    pub fn catalog_exists(&self, name: &str) -> bool {
        FrameRoot::parse_name(name)
            .map(|n| self.catalog_path(&n).is_file())
            .unwrap_or(false)
    }

    pub fn list_catalogs(&self) -> Result<Vec<Catalog>, FrameError> {
        let mut out = Vec::new();
        let dir = self.root.join("catalogs");
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

    pub fn catalog(&self, name: &str) -> Result<Catalog, FrameError> {
        let name = Self::parse_name(name)?;
        let path = self.catalog_path(&name);
        if !path.is_file() {
            return Err(FrameError::UnknownCatalog(name));
        }
        self.read_json(&path)
    }

    pub fn create_catalog(&self, name: &str) -> Result<Catalog, FrameError> {
        let name = Self::parse_name(name)?;
        if self.catalog_exists(&name) {
            return Err(FrameError::CatalogExists(name));
        }
        let cat = Catalog {
            name,
            workspaces: Vec::new(),
        };
        self.save_catalog(&cat)?;
        Ok(cat)
    }

    pub fn save_catalog(&self, cat: &Catalog) -> Result<(), FrameError> {
        let name = Self::parse_name(&cat.name)?;
        self.write_json(&self.catalog_path(&name), cat)
    }

    pub fn delete_catalog(&self, name: &str) -> Result<(), FrameError> {
        let name = Self::parse_name(name)?;
        let path = self.catalog_path(&name);
        if !path.is_file() {
            return Err(FrameError::UnknownCatalog(name));
        }
        fs::remove_file(path)?;
        Ok(())
    }
}
