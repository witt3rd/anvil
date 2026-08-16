//! Left rail: catalogs, workspaces, members. Chrome of the layout, not a member.

use crate::frame::{FrameError, FrameRoot, MemberRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Rail,
    Compose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Naming {
    Session(String),
    Pty(String),
    Edit(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailKind {
    Catalog,
    Workspace,
    Member,
}

#[derive(Debug, Clone)]
pub struct Rail {
    pub catalog: String,
    pub workspace: String,
    pub session: String,
    pub catalogs: Vec<String>,
    pub workspaces: Vec<String>,
    pub members: Vec<String>,
    pub ptys: Vec<String>,
    pub clocks: Vec<String>,
    pub logs: Vec<(String, String)>,
    pub edits: Vec<String>,
    pub weights: Vec<u16>,
    pub kind: RailKind,
    pub idx: usize,
    pub naming: Option<Naming>,
    pub layout_name: String,
}

impl Rail {
    pub fn load(
        root: &FrameRoot,
        catalog: Option<&str>,
        workspace: Option<&str>,
        session: Option<&str>,
    ) -> Result<Self, FrameError> {
        root.ensure_defaults()?;
        let layout = if root.layout_exists("default") {
            root.layout("default")?
        } else {
            let layout = crate::frame::Layout::for_catalog("default", "default");
            root.save_layout(&layout)?;
            layout
        };
        let catalog = catalog
            .map(str::to_string)
            .or(Some(layout.catalog.clone()))
            .unwrap_or_else(|| "default".into());
        let cat = if root.catalog_exists(&catalog) {
            root.catalog(&catalog)?
        } else {
            let mut created = root.create_catalog(&catalog)?;
            created.add_workspace("default");
            root.save_catalog(&created)?;
            created
        };
        let workspaces = cat.workspaces.clone();
        let workspace = workspace
            .map(str::to_string)
            .or(layout.front_workspace.clone())
            .or_else(|| workspaces.first().cloned())
            .unwrap_or_else(|| "default".into());
        if !root.workspace_exists(&workspace) {
            let mut ws = root.create_workspace(&workspace)?;
            ws.add_member(MemberRef::session("default"));
            root.save_workspace(&ws)?;
        }
        let ws = root.workspace(&workspace)?;
        let members: Vec<String> = ws.members.iter().map(|m| m.id().to_string()).collect();
        let ptys: Vec<String> = ws
            .members
            .iter()
            .filter(|m| m.is_pty())
            .map(|m| m.id().to_string())
            .collect();
        let clocks: Vec<String> = ws
            .members
            .iter()
            .filter(|m| m.is_clock())
            .map(|m| m.id().to_string())
            .collect();
        let logs: Vec<(String, String)> = ws
            .members
            .iter()
            .filter_map(|m| m.log_of().map(|of| (m.id().to_string(), of.to_string())))
            .collect();
        let edits: Vec<String> = ws
            .members
            .iter()
            .filter(|m| m.is_edit())
            .map(|m| m.id().to_string())
            .collect();
        let session = session
            .map(str::to_string)
            .or(layout.front_session.clone())
            .or_else(|| members.first().cloned())
            .unwrap_or_else(|| "default".into());
        let named = ptys.iter().any(|p| p == &session)
            || clocks.iter().any(|c| c == &session)
            || logs.iter().any(|(id, _)| id == &session)
            || edits.iter().any(|e| e == &session);
        if !named && !root.session_exists(&session) {
            root.create_session(&session)?;
        }
        if !members.iter().any(|m| m == &session) {
            let mut ws = root.workspace(&workspace)?;
            ws.add_member(MemberRef::session(&session));
            root.save_workspace(&ws)?;
        }
        let mut rail = Self {
            catalog,
            workspace,
            session,
            catalogs: root.list_catalogs()?.into_iter().map(|c| c.name).collect(),
            workspaces,
            members,
            ptys,
            clocks,
            logs,
            edits,
            weights: layout.weights.clone(),
            kind: RailKind::Member,
            idx: 0,
            naming: None,
            layout_name: layout.name,
        };
        rail.refresh(root)?;
        rail.idx = rail
            .members
            .iter()
            .position(|m| m == &rail.session)
            .unwrap_or(0);
        Ok(rail)
    }

    pub fn refresh(&mut self, root: &FrameRoot) -> Result<(), FrameError> {
        self.catalogs = root.list_catalogs()?.into_iter().map(|c| c.name).collect();
        if root.catalog_exists(&self.catalog) {
            self.workspaces = root.catalog(&self.catalog)?.workspaces;
        } else {
            self.workspaces.clear();
        }
        if root.workspace_exists(&self.workspace) {
            let ms = root.workspace(&self.workspace)?.members;
            self.members = ms.iter().map(|m| m.id().to_string()).collect();
            self.ptys = ms
                .iter()
                .filter(|m| m.is_pty())
                .map(|m| m.id().to_string())
                .collect();
            self.clocks = ms
                .iter()
                .filter(|m| m.is_clock())
                .map(|m| m.id().to_string())
                .collect();
            self.logs = ms
                .iter()
                .filter_map(|m| m.log_of().map(|of| (m.id().to_string(), of.to_string())))
                .collect();
            self.edits = ms
                .iter()
                .filter(|m| m.is_edit())
                .map(|m| m.id().to_string())
                .collect();
        } else {
            self.members.clear();
            self.ptys.clear();
            self.clocks.clear();
            self.logs.clear();
            self.edits.clear();
        }
        self.clamp();
        Ok(())
    }

    pub fn reclamp(&mut self) {
        self.clamp();
    }

    fn clamp(&mut self) {
        let len = self.current_list().len();
        if len == 0 {
            self.idx = 0;
        } else {
            self.idx = self.idx.min(len - 1);
        }
    }

    pub fn current_list(&self) -> &[String] {
        match self.kind {
            RailKind::Catalog => &self.catalogs,
            RailKind::Workspace => &self.workspaces,
            RailKind::Member => &self.members,
        }
    }

    pub fn move_idx(&mut self, delta: isize) {
        let len = self.current_list().len();
        if len == 0 {
            self.idx = 0;
            return;
        }
        let next = self.idx as isize + delta;
        self.idx = next.clamp(0, (len - 1) as isize) as usize;
    }

    /// Cycle the sash (workspace) in the current catalog. Returns true if
    /// the focused session changed.
    pub fn cycle_sash(&mut self, root: &FrameRoot, delta: isize) -> Result<bool, FrameError> {
        self.refresh(root)?;
        if self.workspaces.is_empty() {
            return Ok(false);
        }
        let i = self
            .workspaces
            .iter()
            .position(|w| w == &self.workspace)
            .unwrap_or(0);
        let n = self.workspaces.len() as isize;
        let next = (i as isize + delta).rem_euclid(n) as usize;
        if self.workspaces[next] == self.workspace {
            return Ok(false);
        }
        self.workspace = self.workspaces[next].clone();
        self.refresh(root)?;
        let switched = if self.members.iter().any(|m| m == &self.session) {
            false
        } else if let Some(first) = self.members.first().cloned() {
            self.session = first;
            true
        } else {
            false
        };
        self.kind = RailKind::Member;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)?;
        Ok(switched)
    }

    #[allow(dead_code)]
    pub fn peer_session(&self) -> Option<String> {
        self.other_members().into_iter().next()
    }

    pub fn other_members(&self) -> Vec<String> {
        self.members
            .iter()
            .filter(|m| *m != &self.session)
            .cloned()
            .collect()
    }

    #[allow(dead_code)]
    pub fn focus_peer(&mut self, root: &FrameRoot) -> Result<bool, FrameError> {
        self.cycle_member(root, 1)
    }

    pub fn cycle_member(&mut self, root: &FrameRoot, delta: isize) -> Result<bool, FrameError> {
        if self.members.len() < 2 {
            return Ok(false);
        }
        let i = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        let n = self.members.len() as isize;
        let next = (i as isize + delta).rem_euclid(n) as usize;
        if self.members[next] == self.session {
            return Ok(false);
        }
        self.session = self.members[next].clone();
        self.idx = next;
        self.persist(root)?;
        Ok(true)
    }

    pub fn focused_is_pty(&self) -> bool {
        self.ptys.iter().any(|p| p == &self.session)
    }

    pub fn focused_is_edit(&self) -> bool {
        self.edits.iter().any(|e| e == &self.session)
    }

    pub fn member_label(&self, id: &str) -> String {
        if self.ptys.iter().any(|p| p == id) {
            format!("{id} · pty")
        } else if self.edits.iter().any(|e| e == id) {
            format!("{id} · edit")
        } else if self.clocks.iter().any(|c| c == id) {
            format!("{id} · clock")
        } else if let Some((_, of)) = self.logs.iter().find(|(lid, _)| lid == id) {
            format!("{id} · log {of}")
        } else {
            id.to_string()
        }
    }

    pub fn member_is_clock(&self, id: &str) -> bool {
        self.clocks.iter().any(|c| c == id)
    }

    pub fn member_is_log(&self, id: &str) -> bool {
        self.logs.iter().any(|(lid, _)| lid == id)
    }

    pub fn log_of(&self, id: &str) -> Option<&str> {
        self.logs
            .iter()
            .find(|(lid, _)| lid == id)
            .map(|(_, of)| of.as_str())
    }

    pub fn stage_members(&self) -> Vec<String> {
        self.members
            .iter()
            .filter(|id| !self.member_is_clock(id))
            .cloned()
            .collect()
    }

    pub fn bump_weight(&mut self, root: &FrameRoot, delta: i16) -> Result<(), FrameError> {
        let stage = self.stage_members();
        if self.weights.len() != stage.len() {
            self.weights = vec![1; stage.len().max(1)];
        }
        if let Some(i) = stage.iter().position(|m| m == &self.session) {
            let next = (self.weights[i] as i16 + delta).clamp(1, 20) as u16;
            self.weights[i] = next;
        }
        self.persist(root)
    }

    pub fn create_pty(&mut self, root: &FrameRoot, name: &str) -> Result<(), FrameError> {
        let name = FrameRoot::parse_name(name)?;
        if !root.workspace_exists(&self.workspace) {
            root.create_workspace(&self.workspace)?;
        }
        let mut ws = root.workspace(&self.workspace)?;
        ws.add_member(MemberRef::pty(&name));
        root.save_workspace(&ws)?;
        self.session = name;
        self.kind = RailKind::Member;
        self.refresh(root)?;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)?;
        Ok(())
    }

    pub fn create_edit(&mut self, root: &FrameRoot, name: &str) -> Result<(), FrameError> {
        let name = if name.trim().is_empty() {
            root.mint_name()?
        } else {
            FrameRoot::parse_name(name)?
        };
        if !root.workspace_exists(&self.workspace) {
            root.create_workspace(&self.workspace)?;
        }
        let mut ws = root.workspace(&self.workspace)?;
        ws.add_member(MemberRef::edit(&name));
        root.save_workspace(&ws)?;
        self.session = name;
        self.kind = RailKind::Member;
        self.refresh(root)?;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)?;
        Ok(())
    }

    pub fn cycle_kind(&mut self) {
        self.kind = match self.kind {
            RailKind::Catalog => RailKind::Workspace,
            RailKind::Workspace => RailKind::Member,
            RailKind::Member => RailKind::Catalog,
        };
        self.clamp();
    }

    pub fn apply_enter(&mut self, root: &FrameRoot) -> Result<bool, FrameError> {
        let Some(name) = self.current_list().get(self.idx).cloned() else {
            return Ok(false);
        };
        match self.kind {
            RailKind::Catalog => {
                self.catalog = name;
                self.refresh(root)?;
                if let Some(first) = self.workspaces.first().cloned() {
                    self.workspace = first;
                    self.refresh(root)?;
                }
                self.kind = RailKind::Workspace;
                self.idx = 0;
                self.persist(root)?;
                Ok(false)
            }
            RailKind::Workspace => {
                self.workspace = name;
                self.refresh(root)?;
                self.kind = RailKind::Member;
                self.idx = self
                    .members
                    .iter()
                    .position(|m| m == &self.session)
                    .unwrap_or(0);
                if let Some(first) = self.members.first().cloned() {
                    if !self.members.iter().any(|m| m == &self.session) {
                        self.session = first;
                        self.persist(root)?;
                        return Ok(true);
                    }
                }
                self.persist(root)?;
                Ok(false)
            }
            RailKind::Member => {
                if self.session == name {
                    return Ok(false);
                }
                self.session = name;
                self.persist(root)?;
                Ok(true)
            }
        }
    }

    pub fn create_session(&mut self, root: &FrameRoot, name: &str) -> Result<(), FrameError> {
        let name = if name.trim().is_empty() {
            root.mint_name()?
        } else {
            FrameRoot::parse_name(name)?
        };
        if !root.session_exists(&name) {
            root.create_session(&name)?;
        }
        if !root.workspace_exists(&self.workspace) {
            root.create_workspace(&self.workspace)?;
        }
        let mut ws = root.workspace(&self.workspace)?;
        ws.add_member(MemberRef::session(&name));
        root.save_workspace(&ws)?;
        if root.catalog_exists(&self.catalog) {
            let mut cat = root.catalog(&self.catalog)?;
            if !cat.workspaces.iter().any(|w| w == &self.workspace) {
                cat.add_workspace(&self.workspace);
                root.save_catalog(&cat)?;
            }
        }
        self.session = name;
        self.kind = RailKind::Member;
        self.refresh(root)?;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)?;
        Ok(())
    }

    pub fn persist(&self, root: &FrameRoot) -> Result<(), FrameError> {
        let mut layout = if root.layout_exists(&self.layout_name) {
            root.layout(&self.layout_name)?
        } else {
            crate::frame::Layout::for_catalog(&self.layout_name, &self.catalog)
        };
        layout.catalog = self.catalog.clone();
        layout.front_workspace = Some(self.workspace.clone());
        layout.front_session = Some(self.session.clone());
        layout.weights = self.weights.clone();
        root.save_layout(&layout)
    }

    pub fn create_clock(&mut self, root: &FrameRoot) -> Result<(), FrameError> {
        if !root.workspace_exists(&self.workspace) {
            root.create_workspace(&self.workspace)?;
        }
        let mut ws = root.workspace(&self.workspace)?;
        ws.add_member(MemberRef::clock("clock"));
        root.save_workspace(&ws)?;
        self.refresh(root)?;
        self.persist(root)
    }

    pub fn create_log(&mut self, root: &FrameRoot, of: &str) -> Result<(), FrameError> {
        let of = FrameRoot::parse_name(of)?;
        let id = format!("{of}-log");
        if !root.workspace_exists(&self.workspace) {
            root.create_workspace(&self.workspace)?;
        }
        let mut ws = root.workspace(&self.workspace)?;
        ws.add_member(MemberRef::log(&id, &of));
        root.save_workspace(&ws)?;
        self.session = id;
        self.kind = RailKind::Member;
        self.refresh(root)?;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_seeds_defaults_and_create_session_joins_workspace() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        assert_eq!(rail.session, "default");
        assert_eq!(rail.workspace, "default");
        rail.create_session(&root, "audit").unwrap();
        assert_eq!(rail.session, "audit");
        let ws = root.workspace("default").unwrap();
        assert!(ws.members.iter().any(|m| m.session_id() == Some("audit")));
        let layout = root.layout("default").unwrap();
        assert_eq!(layout.front_session.as_deref(), Some("audit"));
    }

    #[test]
    fn cycle_sash_switches_workspace_and_peer_is_the_other_member() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        root.ensure_defaults().unwrap();
        root.create_session("audit").unwrap();
        root.create_session("research").unwrap();
        let mut fleet = root.create_workspace("fleet-os").unwrap();
        fleet.add_member(MemberRef::session("audit"));
        fleet.add_member(MemberRef::session("research"));
        root.save_workspace(&fleet).unwrap();
        let mut cat = root.catalog("default").unwrap();
        cat.add_workspace("fleet-os");
        root.save_catalog(&cat).unwrap();
        let mut rail =
            Rail::load(&root, Some("default"), Some("default"), Some("default")).unwrap();
        assert_eq!(rail.workspace, "default");
        let switched = rail.cycle_sash(&root, 1).unwrap();
        assert_eq!(rail.workspace, "fleet-os");
        assert!(switched);
        assert_eq!(rail.session, "audit");
        assert_eq!(rail.peer_session().as_deref(), Some("research"));
        assert!(rail.focus_peer(&root).unwrap());
        assert_eq!(rail.session, "research");
        assert_eq!(rail.peer_session().as_deref(), Some("audit"));
    }

    #[test]
    fn other_members_and_cycle_visit_every_member() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_session(&root, "audit").unwrap();
        rail.create_pty(&root, "bash").unwrap();
        rail.session = "default".into();
        rail.refresh(&root).unwrap();
        assert_eq!(
            rail.other_members(),
            vec!["audit".to_string(), "bash".into()]
        );
        assert!(rail.cycle_member(&root, 1).unwrap());
        assert_eq!(rail.session, "audit");
        assert!(rail.cycle_member(&root, 1).unwrap());
        assert_eq!(rail.session, "bash");
        assert!(rail.cycle_member(&root, 1).unwrap());
        assert_eq!(rail.session, "default");
        assert!(rail.cycle_member(&root, -1).unwrap());
        assert_eq!(rail.session, "bash");
    }

    #[test]
    fn create_pty_joins_workspace_without_a_session() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_pty(&root, "bash").unwrap();
        assert_eq!(rail.session, "bash");
        assert!(rail.focused_is_pty());
        assert!(!root.session_exists("bash"));
        let ws = root.workspace("default").unwrap();
        assert!(ws.members.iter().any(|m| m == &MemberRef::pty("bash")));
        let layout = root.layout("default").unwrap();
        assert_eq!(layout.front_session.as_deref(), Some("bash"));
    }

    #[test]
    fn create_edit_joins_workspace_without_a_session() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_edit(&root, "notes").unwrap();
        assert_eq!(rail.session, "notes");
        assert!(rail.focused_is_edit());
        assert!(!root.session_exists("notes"));
        let ws = root.workspace("default").unwrap();
        assert!(ws.members.iter().any(|m| m == &MemberRef::edit("notes")));
    }
}
