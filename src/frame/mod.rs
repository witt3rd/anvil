//! Conceptual objects on disk: session, workspace, catalog. Layout is UI persist.
//!
//! Destroying a collection does not destroy its occupants. Occupants may
//! appear in any number of collections.

mod catalog;
mod layout;
mod log;
mod session;
mod transcript;
mod workspace;

pub use catalog::Catalog;
pub use layout::Layout;
pub use log::{Event, EventBody};
pub use session::{SessionMeta, SessionRef};
pub use transcript::TranscriptLine;
pub use workspace::{MemberRef, Workspace};

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::dirs_home;

const NAME_MAX: usize = 64;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("invalid name '{0}': use a letter, then letters, digits, space, _ or -")]
    BadName(String),
    #[error("unknown session '{0}'")]
    UnknownSession(String),
    #[error("unknown workspace '{0}'")]
    UnknownWorkspace(String),
    #[error("unknown catalog '{0}'")]
    UnknownCatalog(String),
    #[error("unknown layout '{0}'")]
    UnknownLayout(String),
    #[error("session '{0}' already exists")]
    SessionExists(String),
    #[error("workspace '{0}' already exists")]
    WorkspaceExists(String),
    #[error("catalog '{0}' already exists")]
    CatalogExists(String),
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json {path}: {source}", path = .path.display())]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone)]
pub struct FrameRoot {
    root: PathBuf,
}

impl FrameRoot {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, FrameError> {
        let root = root.into();
        fs::create_dir_all(root.join("sessions"))?;
        fs::create_dir_all(root.join("workspaces"))?;
        fs::create_dir_all(root.join("catalogs"))?;
        fs::create_dir_all(root.join("layouts"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_defaults(&self) -> Result<SessionRef, FrameError> {
        let session = if self.session_exists("default") {
            let session = self.session("default")?;
            if !session.dir.join("meta.json").is_file() {
                self.save_session_meta(&session.meta)?;
            }
            session
        } else {
            self.create_session("default")?
        };
        if !self.workspace_exists("default") {
            let mut ws = self.create_workspace("default")?;
            ws.add_member(MemberRef::session(&session.id));
            self.save_workspace(&ws)?;
        } else {
            let mut ws = self.workspace("default")?;
            if !ws.members.iter().any(|m| m.session_id() == Some("default")) {
                ws.add_member(MemberRef::session("default"));
                self.save_workspace(&ws)?;
            }
        }
        if !self.catalog_exists("default") {
            let mut cat = self.create_catalog("default")?;
            cat.add_workspace("default");
            self.save_catalog(&cat)?;
        } else {
            let mut cat = self.catalog("default")?;
            if !cat.workspaces.iter().any(|w| w == "default") {
                cat.add_workspace("default");
                self.save_catalog(&cat)?;
            }
        }
        if !self.layout_exists("default") {
            self.save_layout(&Layout::for_catalog("default", "default"))?;
        }
        Ok(session)
    }

    pub fn parse_name(raw: &str) -> Result<String, FrameError> {
        let name = raw.trim();
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(FrameError::BadName(raw.into()));
        }
        if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
            return Err(FrameError::BadName(raw.into()));
        }
        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            return Err(FrameError::BadName(raw.into()));
        };
        if !first.is_ascii_alphabetic() {
            return Err(FrameError::BadName(raw.into()));
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ' ') {
            return Err(FrameError::BadName(raw.into()));
        }
        Ok(name.to_string())
    }

    fn write_json<T: serde::Serialize>(&self, path: &Path, value: &T) -> Result<(), FrameError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(value).expect("frame types are serializable");
        fs::write(path, data)?;
        Ok(())
    }

    fn read_json<T: serde::de::DeserializeOwned>(&self, path: &Path) -> Result<T, FrameError> {
        let data = fs::read(path)?;
        serde_json::from_slice(&data).map_err(|source| FrameError::Json {
            path: path.to_path_buf(),
            source,
        })
    }
}

pub fn default_root() -> PathBuf {
    if let Ok(dir) = std::env::var("ANVIL_ROOT") {
        return PathBuf::from(dir);
    }
    dirs_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".anvil")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp() -> (TempDir, FrameRoot) {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        (dir, root)
    }

    #[test]
    fn name_rejects_path_bits_and_empty() {
        assert!(FrameRoot::parse_name("").is_err());
        assert!(FrameRoot::parse_name("../x").is_err());
        assert!(FrameRoot::parse_name("foo/bar").is_err());
        assert!(FrameRoot::parse_name("1fox").is_err());
        assert_eq!(FrameRoot::parse_name(" fleet-os ").unwrap(), "fleet-os");
        assert_eq!(
            FrameRoot::parse_name("home system management").unwrap(),
            "home system management"
        );
    }

    #[test]
    fn defaults_are_idempotent_and_linked() {
        let (_dir, root) = tmp();
        root.ensure_defaults().unwrap();
        root.ensure_defaults().unwrap();
        let ws = root.workspace("default").unwrap();
        assert_eq!(ws.members, vec![MemberRef::session("default")]);
        let cat = root.catalog("default").unwrap();
        assert_eq!(cat.workspaces, vec!["default".to_string()]);
        assert_eq!(root.layout("default").unwrap().catalog, "default");
        assert!(root.session_dir("default").join("meta.json").is_file());
    }

    #[test]
    fn destroy_workspace_keeps_sessions() {
        let (_dir, root) = tmp();
        root.create_session("audit").unwrap();
        let mut ws = root.create_workspace("fleet-os").unwrap();
        ws.add_member(MemberRef::session("audit"));
        root.save_workspace(&ws).unwrap();
        root.delete_workspace("fleet-os").unwrap();
        assert!(!root.workspace_exists("fleet-os"));
        assert!(root.session_exists("audit"));
    }

    #[test]
    fn destroy_catalog_keeps_workspaces() {
        let (_dir, root) = tmp();
        root.create_workspace("fleet-os").unwrap();
        let mut cat = root.create_catalog("compute saturation").unwrap();
        cat.add_workspace("fleet-os");
        root.save_catalog(&cat).unwrap();
        root.delete_catalog("compute saturation").unwrap();
        assert!(!root.catalog_exists("compute saturation"));
        assert!(root.workspace_exists("fleet-os"));
    }

    #[test]
    fn many_to_many_membership() {
        let (_dir, root) = tmp();
        root.create_session("audit").unwrap();
        let mut a = root.create_workspace("fleet-os").unwrap();
        let mut b = root.create_workspace("weekly").unwrap();
        a.add_member(MemberRef::session("audit"));
        b.add_member(MemberRef::session("audit"));
        root.save_workspace(&a).unwrap();
        root.save_workspace(&b).unwrap();
        let mut home = root.create_catalog("home system management").unwrap();
        let mut compute = root.create_catalog("compute saturation").unwrap();
        home.add_workspace("fleet-os");
        compute.add_workspace("fleet-os");
        root.save_catalog(&home).unwrap();
        root.save_catalog(&compute).unwrap();
        assert_eq!(root.workspace("fleet-os").unwrap().members.len(), 1);
        assert_eq!(
            root.catalog("home system management").unwrap().workspaces,
            ["fleet-os"]
        );
        assert_eq!(
            root.catalog("compute saturation").unwrap().workspaces,
            ["fleet-os"]
        );
    }

    #[test]
    fn legacy_default_store_is_the_default_session() {
        let dir = TempDir::new().unwrap();
        let legacy = dir.path().join("default");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("namespace.pkl"), b"pickle").unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let session = root.ensure_defaults().unwrap();
        assert_eq!(session.dir, legacy);
        assert!(legacy.join("meta.json").is_file());
        assert!(!dir.path().join("sessions/default/namespace.pkl").exists());
    }

    #[test]
    fn transcript_round_trips() {
        let (_dir, root) = tmp();
        root.create_session("fox").unwrap();
        root.append_transcript("fox", &TranscriptLine::User { text: "hi".into() })
            .unwrap();
        root.append_transcript("fox", &TranscriptLine::Answer { text: "yo".into() })
            .unwrap();
        let lines = root.load_transcript("fox").unwrap();
        assert_eq!(lines.len(), 2);
        match &lines[0] {
            TranscriptLine::User { text } => assert_eq!(text, "hi"),
            _ => panic!("{lines:?}"),
        }
    }
}
