//! Left rail: catalogs, workspaces, members. Chrome of the layout, not a member.

use std::collections::HashSet;

use crate::frame::{apply_gap, clamp_weight, FrameError, FrameRoot, MemberRef, SplitDir, Tile};

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
    Tab(String),
    Catalog(String),
    RenameTab { name: String, buf: String },
    RenameCatalog(String),
    RenamePane { id: String, buf: String },
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
    pub plots: Vec<(String, String)>,
    pub weights: Vec<u16>,
    pub tiles: Option<Tile>,
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
        let plots: Vec<(String, String)> = ws
            .members
            .iter()
            .filter_map(|m| m.plot_of().map(|of| (m.id().to_string(), of.to_string())))
            .collect();
        let session = session
            .map(str::to_string)
            .or(layout.front_session.clone())
            .or_else(|| members.first().cloned())
            .unwrap_or_else(|| "default".into());
        let named = ptys.iter().any(|p| p == &session)
            || clocks.iter().any(|c| c == &session)
            || logs.iter().any(|(id, _)| id == &session)
            || edits.iter().any(|e| e == &session)
            || plots.iter().any(|(id, _)| id == &session);
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
            workspace: workspace.clone(),
            session,
            catalogs: root.list_catalogs()?.into_iter().map(|c| c.name).collect(),
            workspaces,
            members,
            ptys,
            clocks,
            logs,
            edits,
            plots,
            weights: layout.weights.clone(),
            tiles: layout.tiles.get(&workspace).cloned(),
            kind: RailKind::Member,
            idx: 0,
            naming: None,
            layout_name: layout.name,
        };
        rail.refresh(root)?;
        rail.ensure_tiles();
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
            self.plots = ms
                .iter()
                .filter_map(|m| m.plot_of().map(|of| (m.id().to_string(), of.to_string())))
                .collect();
        } else {
            self.members.clear();
            self.ptys.clear();
            self.clocks.clear();
            self.logs.clear();
            self.edits.clear();
            self.plots.clear();
        }
        self.clamp();
        Ok(())
    }

    #[allow(dead_code)]
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

    /// Jump onto `kind`. Already-on-kind keeps the highlight so the
    /// user can keep walking; entering snaps to the live selection.
    pub fn enter_kind(&mut self, kind: RailKind) {
        if self.kind == kind {
            return;
        }
        self.kind = kind;
        self.idx = match kind {
            RailKind::Catalog => self
                .catalogs
                .iter()
                .position(|c| c == &self.catalog)
                .unwrap_or(0),
            RailKind::Workspace => self
                .workspaces
                .iter()
                .position(|w| w == &self.workspace)
                .unwrap_or(0),
            RailKind::Member => self
                .members
                .iter()
                .position(|m| m == &self.session)
                .unwrap_or(0),
        };
    }

    /// Walk members. Returns the highlighted id; caller peeks or selects.
    pub fn step_member(&mut self, delta: isize) -> Option<String> {
        self.enter_kind(RailKind::Member);
        self.move_idx(delta);
        self.current_list().get(self.idx).cloned()
    }

    /// Walk workspaces. Highlight only — does not switch the sash.
    pub fn step_workspace(&mut self, delta: isize) -> Option<String> {
        self.enter_kind(RailKind::Workspace);
        self.move_idx(delta);
        self.current_list().get(self.idx).cloned()
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
        self.adopt_tiles(root);
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

    /// Jump to a named workspace sash. Returns true if the focused member
    /// had to change (it was not in the target bench).
    pub fn select_workspace(&mut self, root: &FrameRoot, name: &str) -> Result<bool, FrameError> {
        self.refresh(root)?;
        if !self.workspaces.iter().any(|w| w == name) {
            return Ok(false);
        }
        if self.workspace == name {
            self.kind = RailKind::Workspace;
            self.idx = self.workspaces.iter().position(|w| w == name).unwrap_or(0);
            return Ok(false);
        }
        self.workspace = name.to_string();
        self.refresh(root)?;
        self.adopt_tiles(root);
        let switched = if self.members.iter().any(|m| m == &self.session) {
            false
        } else if let Some(first) = self.members.first().cloned() {
            self.session = first;
            true
        } else {
            false
        };
        self.kind = RailKind::Workspace;
        self.idx = self.workspaces.iter().position(|w| w == name).unwrap_or(0);
        self.persist(root)?;
        Ok(switched)
    }

    /// Focus a member in the current workspace. Returns true if the
    /// front member changed.
    pub fn select_member(&mut self, root: &FrameRoot, id: &str) -> Result<bool, FrameError> {
        if !self.peek_member(id) {
            return Ok(false);
        }
        self.persist(root)?;
        Ok(true)
    }

    /// Point the rail at a member without writing the layout. Hover uses
    /// this so crossing a pane does not flush disk.
    pub fn peek_member(&mut self, id: &str) -> bool {
        if !self.members.iter().any(|m| m == id) {
            return false;
        }
        self.kind = RailKind::Member;
        self.idx = self.members.iter().position(|m| m == id).unwrap_or(0);
        if self.session == id {
            return false;
        }
        self.session = id.to_string();
        true
    }

    pub fn peek_row(&mut self, kind: RailKind, name: &str) {
        self.kind = kind;
        let list = self.current_list();
        if let Some(i) = list.iter().position(|n| n == name) {
            self.idx = i;
        }
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
        } else if let Some((_, of)) = self.plots.iter().find(|(pid, _)| pid == id) {
            format!("{id} · stats {of}")
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

    pub fn member_is_plot(&self, id: &str) -> bool {
        self.plots.iter().any(|(pid, _)| pid == id)
    }

    pub fn plot_of(&self, id: &str) -> Option<&str> {
        self.plots
            .iter()
            .find(|(pid, _)| pid == id)
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
        self.ensure_tiles();
        if let Some(t) = self.tiles.as_mut() {
            t.bump_weight(&self.session, delta);
        } else {
            let stage = self.stage_members();
            if self.weights.len() != stage.len() {
                self.weights = vec![1; stage.len().max(1)];
            }
            if let Some(i) = stage.iter().position(|m| m == &self.session) {
                self.weights[i] = clamp_weight(self.weights[i], delta);
            }
        }
        self.persist(root)
    }

    fn ensure_tiles(&mut self) {
        let stage = self.stage_members();
        match self.tiles.as_mut() {
            None => self.tiles = Tile::from_stage(&stage, &self.weights),
            Some(t) => {
                let keep: HashSet<String> = stage.iter().cloned().collect();
                if !t.prune(&keep) {
                    self.tiles = Tile::from_stage(&stage, &self.weights);
                } else {
                    t.sync_stage(&stage);
                }
            }
        }
    }

    fn insert_tile(&mut self, focus: &str, new_id: &str, dir: SplitDir) {
        if self.tiles.is_none() {
            let prior: Vec<String> = self
                .stage_members()
                .into_iter()
                .filter(|id| id != new_id)
                .collect();
            self.tiles = Tile::from_stage(&prior, &self.weights);
        }
        let stage = self.stage_members();
        if let Some(t) = self.tiles.as_mut() {
            if !t.split(focus, new_id, dir) {
                t.sync_stage(&stage);
            }
        }
    }

    /// Herdr split: mint a PTY and bisect the focused pane.
    pub fn split_pane(&mut self, root: &FrameRoot, dir: SplitDir) -> Result<String, FrameError> {
        let focus = self.session.clone();
        let name = root.mint_name()?;
        if !root.workspace_exists(&self.workspace) {
            root.create_workspace(&self.workspace)?;
        }
        let mut ws = root.workspace(&self.workspace)?;
        ws.add_member(MemberRef::pty(&name));
        root.save_workspace(&ws)?;
        self.refresh(root)?;
        self.insert_tile(&focus, &name, dir);
        self.session = name.clone();
        self.kind = RailKind::Member;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)?;
        Ok(name)
    }

    pub fn create_pty(&mut self, root: &FrameRoot, name: &str) -> Result<(), FrameError> {
        let focus = self.session.clone();
        let name = if name.trim().is_empty() {
            root.mint_name()?
        } else {
            FrameRoot::parse_name(name)?
        };
        if !root.workspace_exists(&self.workspace) {
            root.create_workspace(&self.workspace)?;
        }
        let mut ws = root.workspace(&self.workspace)?;
        ws.add_member(MemberRef::pty(&name));
        root.save_workspace(&ws)?;
        self.refresh(root)?;
        self.insert_tile(&focus, &name, SplitDir::Col);
        self.session = name;
        self.kind = RailKind::Member;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)?;
        Ok(())
    }

    pub fn create_edit(&mut self, root: &FrameRoot, name: &str) -> Result<(), FrameError> {
        let focus = self.session.clone();
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
        self.refresh(root)?;
        self.insert_tile(&focus, &name, SplitDir::Col);
        self.session = name;
        self.kind = RailKind::Member;
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
        let focus = self.session.clone();
        self.refresh(root)?;
        self.insert_tile(&focus, &name, SplitDir::Col);
        self.session = name;
        self.kind = RailKind::Member;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)?;
        Ok(())
    }

    pub fn persist(&mut self, root: &FrameRoot) -> Result<(), FrameError> {
        self.ensure_tiles();
        let mut layout = if root.layout_exists(&self.layout_name) {
            root.layout(&self.layout_name)?
        } else {
            crate::frame::Layout::for_catalog(&self.layout_name, &self.catalog)
        };
        layout.catalog = self.catalog.clone();
        layout.front_workspace = Some(self.workspace.clone());
        layout.front_session = Some(self.session.clone());
        layout.weights = self.weights.clone();
        if let Some(t) = &self.tiles {
            layout.tiles.insert(self.workspace.clone(), t.clone());
        } else {
            layout.tiles.remove(&self.workspace);
        }
        root.save_layout(&layout)
    }

    pub fn seed_split_weights(&mut self, path: Option<&[usize]>, sizes: &[u16]) {
        match path {
            None => {
                self.weights = sizes.iter().map(|s| (*s).max(1)).collect();
            }
            Some(path) => {
                self.ensure_tiles();
                if let Some(t) = self.tiles.as_mut() {
                    if let Some(node) = t.at_mut(path) {
                        node.seed_weights(sizes);
                    }
                }
            }
        }
    }

    pub fn apply_split_gap(
        &mut self,
        path: Option<&[usize]>,
        gap: usize,
        px_a: u16,
        px_b: u16,
        delta: i32,
    ) {
        match path {
            None => {
                apply_gap(&mut self.weights, gap, px_a, px_b, delta, 3);
            }
            Some(path) => {
                if let Some(t) = self.tiles.as_mut() {
                    if let Some(node) = t.at_mut(path) {
                        node.set_gap(gap, px_a, px_b, delta);
                    }
                }
            }
        }
    }

    pub fn equalize_split(&mut self, path: Option<&[usize]>) {
        match path {
            None => {
                for w in &mut self.weights {
                    *w = 1;
                }
            }
            Some(path) => {
                if let Some(t) = self.tiles.as_mut() {
                    if let Some(node) = t.at_mut(path) {
                        node.equalize();
                    }
                }
            }
        }
    }

    fn adopt_tiles(&mut self, root: &FrameRoot) {
        self.tiles = root
            .layout(&self.layout_name)
            .ok()
            .and_then(|l| l.tiles.get(&self.workspace).cloned());
        self.ensure_tiles();
    }

    pub fn create_tab(&mut self, root: &FrameRoot, name: &str) -> Result<String, FrameError> {
        let name = if name.trim().is_empty() {
            root.mint_name()?
        } else {
            FrameRoot::parse_name(name)?
        };
        if !root.workspace_exists(&name) {
            let mut ws = root.create_workspace(&name)?;
            if root.session_exists("default") {
                ws.add_member(MemberRef::session("default"));
                root.save_workspace(&ws)?;
            }
        }
        if root.catalog_exists(&self.catalog) {
            let mut cat = root.catalog(&self.catalog)?;
            cat.add_workspace(&name);
            root.save_catalog(&cat)?;
        }
        self.select_workspace(root, &name)?;
        Ok(name)
    }

    pub fn close_tab(&mut self, root: &FrameRoot) -> Result<bool, FrameError> {
        if self.workspaces.len() < 2 {
            return Ok(false);
        }
        let gone = self.workspace.clone();
        if root.catalog_exists(&self.catalog) {
            let mut cat = root.catalog(&self.catalog)?;
            cat.remove_workspace(&gone);
            root.save_catalog(&cat)?;
        }
        self.refresh(root)?;
        if let Some(next) = self.workspaces.first().cloned() {
            self.select_workspace(root, &next)?;
        }
        Ok(true)
    }

    pub fn switch_tab(&mut self, root: &FrameRoot, n: u8) -> Result<bool, FrameError> {
        self.refresh(root)?;
        let idx = n.saturating_sub(1) as usize;
        let Some(name) = self.workspaces.get(idx).cloned() else {
            return Ok(false);
        };
        self.select_workspace(root, &name)
    }

    pub fn create_catalog_front(
        &mut self,
        root: &FrameRoot,
        name: &str,
    ) -> Result<String, FrameError> {
        let name = if name.trim().is_empty() {
            root.mint_name()?
        } else {
            FrameRoot::parse_name(name)?
        };
        if !root.catalog_exists(&name) {
            let mut cat = root.create_catalog(&name)?;
            cat.add_workspace(&self.workspace);
            root.save_catalog(&cat)?;
        }
        self.catalog = name.clone();
        self.refresh(root)?;
        self.persist(root)?;
        Ok(name)
    }

    pub fn close_catalog(&mut self, root: &FrameRoot) -> Result<bool, FrameError> {
        if self.catalogs.len() < 2 {
            return Ok(false);
        }
        let gone = self.catalog.clone();
        let next = self
            .catalogs
            .iter()
            .find(|c| *c != &gone)
            .cloned()
            .unwrap_or_else(|| "default".into());
        self.catalog = next;
        self.refresh(root)?;
        if let Some(first) = self.workspaces.first().cloned() {
            self.select_workspace(root, &first)?;
        } else {
            self.persist(root)?;
        }
        let _ = root.delete_catalog(&gone);
        Ok(true)
    }

    pub fn rename_tab(&mut self, root: &FrameRoot, name: &str) -> Result<String, FrameError> {
        let old = self.workspace.clone();
        self.rename_space(root, &old, name)
    }

    pub fn rename_space(
        &mut self,
        root: &FrameRoot,
        old: &str,
        name: &str,
    ) -> Result<String, FrameError> {
        let was_front = self.workspace == old;
        let new = root.rename_workspace(old, name)?;
        if let Ok(mut layout) = root.layout(&self.layout_name) {
            if let Some(t) = layout.tiles.remove(old) {
                layout.tiles.insert(new.clone(), t);
            }
            if layout.front_workspace.as_deref() == Some(old) {
                layout.front_workspace = Some(new.clone());
            }
            root.save_layout(&layout)?;
        }
        if was_front {
            self.workspace = new.clone();
        }
        self.refresh(root)?;
        self.adopt_tiles(root);
        self.persist(root)?;
        Ok(new)
    }

    pub fn rename_catalog(&mut self, root: &FrameRoot, name: &str) -> Result<String, FrameError> {
        let new = root.rename_catalog(&self.catalog, name)?;
        self.catalog = new.clone();
        self.refresh(root)?;
        self.persist(root)?;
        Ok(new)
    }

    pub fn close_pane(&mut self, root: &FrameRoot) -> Result<bool, FrameError> {
        let stage = self.stage_members();
        if stage.len() < 2 {
            return Ok(false);
        }
        let gone = self.session.clone();
        let next = stage
            .iter()
            .find(|id| *id != &gone)
            .cloned()
            .unwrap_or_else(|| stage[0].clone());
        if root.workspace_exists(&self.workspace) {
            let mut ws = root.workspace(&self.workspace)?;
            ws.members.retain(|m| m.id() != gone);
            root.save_workspace(&ws)?;
        }
        self.refresh(root)?;
        self.ensure_tiles();
        self.session = next;
        self.kind = RailKind::Member;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)?;
        Ok(true)
    }

    pub fn remove_member(&mut self, root: &FrameRoot, id: &str) -> Result<bool, FrameError> {
        if !self.members.iter().any(|m| m == id) {
            return Ok(false);
        }
        let clock = self.member_is_clock(id);
        if !clock && self.stage_members().len() < 2 {
            return Ok(false);
        }
        if root.workspace_exists(&self.workspace) {
            let mut ws = root.workspace(&self.workspace)?;
            ws.members.retain(|m| m.id() != id);
            root.save_workspace(&ws)?;
        }
        let next = if self.session == id {
            self.stage_members()
                .into_iter()
                .find(|m| m != id)
                .or_else(|| self.members.iter().find(|m| *m != id).cloned())
                .unwrap_or_else(|| self.session.clone())
        } else {
            self.session.clone()
        };
        self.refresh(root)?;
        self.ensure_tiles();
        self.session = if self.members.iter().any(|m| m == &next) {
            next
        } else {
            self.members
                .first()
                .cloned()
                .unwrap_or_else(|| self.session.clone())
        };
        self.kind = RailKind::Member;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)?;
        Ok(true)
    }

    pub fn destroy_member(&mut self, root: &FrameRoot, id: &str) -> Result<bool, FrameError> {
        let is_session = root.session_exists(id);
        let is_edit = self.edits.iter().any(|e| e == id);
        if !self.remove_member(root, id)? {
            return Ok(false);
        }
        if is_session {
            self.purge_id(root, id)?;
            let _ = root.delete_session(id);
        }
        if is_edit {
            let path = root.root().join("edits").join(format!("{id}.txt"));
            let _ = std::fs::remove_file(path);
        }
        Ok(true)
    }

    fn purge_id(&mut self, root: &FrameRoot, id: &str) -> Result<(), FrameError> {
        for mut ws in root.list_workspaces()? {
            let before = ws.members.len();
            ws.members.retain(|m| m.id() != id);
            if ws.members.len() != before {
                root.save_workspace(&ws)?;
            }
        }
        self.refresh(root)?;
        self.ensure_tiles();
        self.persist(root)
    }

    pub fn remove_space(&mut self, root: &FrameRoot, name: &str) -> Result<bool, FrameError> {
        if !self.workspaces.iter().any(|w| w == name) {
            return Ok(false);
        }
        if self.workspaces.len() < 2 {
            return Ok(false);
        }
        self.workspace = name.to_string();
        self.close_tab(root)
    }

    pub fn destroy_space(&mut self, root: &FrameRoot, name: &str) -> Result<bool, FrameError> {
        if !self.remove_space(root, name)? {
            return Ok(false);
        }
        let _ = root.delete_workspace(name);
        Ok(true)
    }

    pub fn rename_member(
        &mut self,
        root: &FrameRoot,
        id: &str,
        name: &str,
    ) -> Result<String, FrameError> {
        let new = FrameRoot::parse_name(name)?;
        if id == new {
            return Ok(new);
        }
        for ws in root.list_workspaces()? {
            if ws.members.iter().any(|m| m.id() == new) {
                return Err(FrameError::MemberExists(new));
            }
        }
        if root.session_exists(id) {
            root.rename_session(id, &new)?;
        } else if root.session_exists(&new) {
            return Err(FrameError::MemberExists(new));
        }
        for mut ws in root.list_workspaces()? {
            let mut dirty = false;
            for m in &mut ws.members {
                if m.id() == id {
                    if m.is_edit() {
                        let old = root.root().join("edits").join(format!("{id}.txt"));
                        let next = root.root().join("edits").join(format!("{new}.txt"));
                        if old.is_file() {
                            let _ = std::fs::rename(&old, &next);
                        }
                    }
                    m.set_id(new.clone());
                    dirty = true;
                }
            }
            if dirty {
                root.save_workspace(&ws)?;
            }
        }
        if let Ok(mut layout) = root.layout(&self.layout_name) {
            for t in layout.tiles.values_mut() {
                t.rename_id(id, &new);
            }
            root.save_layout(&layout)?;
        }
        if self.session == id {
            self.session = new.clone();
        }
        self.refresh(root)?;
        self.adopt_tiles(root);
        self.persist(root)?;
        Ok(new)
    }

    pub fn swap_with(&mut self, root: &FrameRoot, other: &str) -> Result<bool, FrameError> {
        if other == self.session {
            return Ok(false);
        }
        self.ensure_tiles();
        if let Some(t) = self.tiles.as_mut() {
            t.swap_ids(&self.session, other);
        }
        self.persist(root)?;
        Ok(true)
    }

    pub fn select_catalog(&mut self, root: &FrameRoot, name: &str) -> Result<bool, FrameError> {
        self.refresh(root)?;
        if !self.catalogs.iter().any(|c| c == name) {
            return Ok(false);
        }
        if self.catalog == name {
            self.kind = RailKind::Catalog;
            return Ok(false);
        }
        self.catalog = name.to_string();
        self.refresh(root)?;
        if let Some(first) = self.workspaces.first().cloned() {
            self.select_workspace(root, &first)?;
        } else {
            self.persist(root)?;
        }
        Ok(true)
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
        let focus = self.session.clone();
        if !root.workspace_exists(&self.workspace) {
            root.create_workspace(&self.workspace)?;
        }
        let mut ws = root.workspace(&self.workspace)?;
        ws.add_member(MemberRef::log(&id, &of));
        root.save_workspace(&ws)?;
        self.refresh(root)?;
        self.insert_tile(&focus, &id, SplitDir::Col);
        self.session = id;
        self.kind = RailKind::Member;
        self.idx = self
            .members
            .iter()
            .position(|m| m == &self.session)
            .unwrap_or(0);
        self.persist(root)
    }

    pub fn create_plot(&mut self, root: &FrameRoot, of: &str) -> Result<(), FrameError> {
        let of = FrameRoot::parse_name(of)?;
        let id = format!("{of}-plot");
        let focus = self.session.clone();
        if !root.workspace_exists(&self.workspace) {
            root.create_workspace(&self.workspace)?;
        }
        let mut ws = root.workspace(&self.workspace)?;
        ws.add_member(MemberRef::plot(&id, &of));
        root.save_workspace(&ws)?;
        self.refresh(root)?;
        self.insert_tile(&focus, &id, SplitDir::Col);
        self.session = id;
        self.kind = RailKind::Member;
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

    #[test]
    fn create_plot_joins_workspace() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_plot(&root, "default").unwrap();
        assert_eq!(rail.session, "default-plot");
        assert!(rail.member_is_plot("default-plot"));
        assert_eq!(rail.plot_of("default-plot"), Some("default"));
        let ws = root.workspace("default").unwrap();
        assert!(ws
            .members
            .iter()
            .any(|m| m == &MemberRef::plot("default-plot", "default")));
        let tiles = rail.tiles.as_ref().unwrap();
        assert!(tiles.leaves().iter().any(|id| *id == "default-plot"));
        let layout = root.layout("default").unwrap();
        assert!(layout
            .tiles
            .get("default")
            .is_some_and(|t| t.leaves().iter().any(|id| *id == "default-plot")));
    }

    #[test]
    fn load_heals_a_plot_missing_from_tiles() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_pty(&root, "bash").unwrap();
        let mut ws = root.workspace("default").unwrap();
        ws.add_member(MemberRef::plot("default-plot", "default"));
        root.save_workspace(&ws).unwrap();
        let rail = Rail::load(&root, None, None, None).unwrap();
        assert!(rail.member_is_plot("default-plot"));
        let tiles = rail.tiles.as_ref().unwrap();
        assert!(tiles.leaves().iter().any(|id| *id == "default-plot"));
        assert!(tiles.leaves().iter().any(|id| *id == "bash"));
    }

    #[test]
    fn select_workspace_and_member_jump_without_cycling() {
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
        assert!(rail.select_workspace(&root, "fleet-os").unwrap());
        assert_eq!(rail.workspace, "fleet-os");
        assert_eq!(rail.session, "audit");
        assert!(rail.select_member(&root, "research").unwrap());
        assert_eq!(rail.session, "research");
        assert!(!rail.select_member(&root, "research").unwrap());
        assert!(!rail.select_workspace(&root, "fleet-os").unwrap());
        assert!(rail.peek_member("audit"));
        assert_eq!(rail.session, "audit");
        assert!(!rail.peek_member("audit"));
    }

    #[test]
    fn step_member_peeks_without_leaving_workspace() {
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
        let mut rail = Rail::load(&root, Some("default"), Some("fleet-os"), Some("audit")).unwrap();
        rail.kind = RailKind::Workspace;
        rail.idx = 0;
        let next = rail.step_member(1).unwrap();
        assert_eq!(next, "research");
        assert_eq!(rail.session, "audit");
        assert_eq!(rail.kind, RailKind::Member);
        assert!(rail.peek_member(&next));
        assert_eq!(rail.session, "research");
        let layout = root.layout("default").unwrap();
        assert_ne!(layout.front_session.as_deref(), Some("research"));
    }

    #[test]
    fn step_workspace_highlights_without_switching() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        root.ensure_defaults().unwrap();
        root.create_workspace("fleet-os").unwrap();
        let mut cat = root.catalog("default").unwrap();
        cat.add_workspace("fleet-os");
        root.save_catalog(&cat).unwrap();
        let mut rail =
            Rail::load(&root, Some("default"), Some("default"), Some("default")).unwrap();
        rail.kind = RailKind::Member;
        let name = rail.step_workspace(1).unwrap();
        assert_eq!(name, "fleet-os");
        assert_eq!(rail.workspace, "default");
        assert_eq!(rail.kind, RailKind::Workspace);
        let name = rail.step_workspace(-1).unwrap();
        assert_eq!(name, "default");
        assert_eq!(rail.workspace, "default");
    }

    #[test]
    fn split_pane_mints_a_pty_beside_the_focus() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        let name = rail.split_pane(&root, SplitDir::Row).unwrap();
        assert!(rail.ptys.iter().any(|p| p == &name));
        assert_eq!(rail.session, name);
        let tiles = rail.tiles.as_ref().unwrap();
        assert_eq!(tiles.leaves(), vec!["default", name.as_str()]);
        match tiles {
            Tile::Split {
                dir: SplitDir::Row, ..
            } => {}
            other => panic!("expected row split, got {other:?}"),
        }
        let layout = root.layout("default").unwrap();
        assert!(layout.tiles.get("default").is_some());
    }

    #[test]
    fn drag_edge_moves_only_the_pair() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_pty(&root, "bash").unwrap();
        rail.seed_split_weights(Some(&[]), &[10, 10]);
        rail.apply_split_gap(Some(&[]), 0, 10, 10, 4);
        match rail.tiles.as_ref() {
            Some(Tile::Split { weights, .. }) => assert_eq!(weights, &vec![14, 6]),
            other => panic!("{other:?}"),
        }
        rail.equalize_split(Some(&[]));
        match rail.tiles.as_ref() {
            Some(Tile::Split { weights, .. }) => assert_eq!(weights, &vec![1, 1]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn remove_member_drops_from_the_bench_and_keeps_the_session() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_session(&root, "audit").unwrap();
        assert!(rail.remove_member(&root, "audit").unwrap());
        assert!(!rail.members.iter().any(|m| m == "audit"));
        assert!(root.session_exists("audit"));
        assert!(!rail.remove_member(&root, "default").unwrap());
        assert!(root.session_exists("default"));
    }

    #[test]
    fn destroy_member_deletes_the_session_dir() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_session(&root, "audit").unwrap();
        assert!(rail.destroy_member(&root, "audit").unwrap());
        assert!(!root.session_exists("audit"));
        assert!(!rail.members.iter().any(|m| m == "audit"));
    }

    #[test]
    fn remove_clock_does_not_need_a_second_pane() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_clock(&root).unwrap();
        assert!(rail.member_is_clock("clock"));
        assert_eq!(rail.stage_members().len(), 1);
        assert!(rail.remove_member(&root, "clock").unwrap());
        assert!(!rail.members.iter().any(|m| m == "clock"));
        assert_eq!(rail.session, "default");
    }

    #[test]
    fn rename_member_renames_the_session_on_disk() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_session(&root, "audit").unwrap();
        let new = rail.rename_member(&root, "audit", "review").unwrap();
        assert_eq!(new, "review");
        assert!(!root.session_exists("audit"));
        assert!(root.session_exists("review"));
        assert!(rail.members.iter().any(|m| m == "review"));
        let tiles = rail.tiles.as_ref().unwrap();
        assert!(tiles.leaves().iter().any(|id| *id == "review"));
        assert!(!tiles.leaves().iter().any(|id| *id == "audit"));
    }

    #[test]
    fn remove_space_drops_the_sash_and_keeps_the_workspace() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_tab(&root, "fleet-os").unwrap();
        assert!(rail.remove_space(&root, "fleet-os").unwrap());
        assert!(!rail.workspaces.iter().any(|w| w == "fleet-os"));
        assert!(root.workspace_exists("fleet-os"));
        assert!(!rail.remove_space(&root, "default").unwrap());
    }

    #[test]
    fn destroy_space_deletes_the_workspace_and_keeps_occupants() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_session(&root, "audit").unwrap();
        rail.create_tab(&root, "fleet-os").unwrap();
        let mut fleet = root.workspace("fleet-os").unwrap();
        fleet.add_member(MemberRef::session("audit"));
        root.save_workspace(&fleet).unwrap();
        assert!(rail.destroy_space(&root, "fleet-os").unwrap());
        assert!(!root.workspace_exists("fleet-os"));
        assert!(root.session_exists("audit"));
        assert!(root.session_exists("default"));
    }

    #[test]
    fn rename_tab_renames_the_front_sash() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_tab(&root, "fleet-os").unwrap();
        let new = rail.rename_tab(&root, "bench").unwrap();
        assert_eq!(new, "bench");
        assert_eq!(rail.workspace, "bench");
        assert!(root.workspace_exists("bench"));
        assert!(!root.workspace_exists("fleet-os"));
    }

    #[test]
    fn rename_space_renames_a_sash_that_is_not_front() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let mut rail = Rail::load(&root, None, None, None).unwrap();
        rail.create_tab(&root, "fleet-os").unwrap();
        rail.select_workspace(&root, "default").unwrap();
        let new = rail.rename_space(&root, "fleet-os", "bench").unwrap();
        assert_eq!(new, "bench");
        assert_eq!(rail.workspace, "default");
        assert!(root.workspace_exists("bench"));
        assert!(!root.workspace_exists("fleet-os"));
        assert!(rail.workspaces.iter().any(|w| w == "bench"));
    }
}
