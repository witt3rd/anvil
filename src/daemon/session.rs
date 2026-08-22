//! The session store. A session is a named group of windows.
//! Kernel: "session — Named group of windows. Does not run." It lives
//! on disk, one directory per session, and reopens from it after a
//! daemon restart. Windows and panes carry the identifiers the daemon
//! issued when it made them.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::acp::{AcpChild, WindowState};
use super::pane::{Grid, Pane};
use super::tiling::Tiling;
use super::watch::HttpWatch;
use crate::catalog::Agents;

const FILE: &str = "session.json";
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// The wire view of a session: its windows, their panes, each pane's
/// geometry, and the focused pane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionView {
    pub windows: Vec<WindowView>,
    pub focused: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowView {
    pub window: String,
    pub panes: Vec<PaneView>,
    #[serde(default)]
    pub state: WindowState,
    /// Markdown blob the operator stores with this window.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneView {
    pub pane: String,
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
    /// Catalog name when this process is an agent. Absent on a shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What this agent is doing: the OpenCode session title, or
    /// the last user line when the title is still a placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
    #[serde(default)]
    pub state: WindowState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Dir {
    Cols,
    Rows,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Tree {
    Leaf { id: String },
    Split {
        dir: Dir,
        a: Box<Tree>,
        b: Box<Tree>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct FileState {
    next_id: u64,
    tty_cols: u16,
    tty_rows: u16,
    windows: Vec<WindowFile>,
    focused: String,
    /// Named agent panes. Spawned again when the session is opened.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    agents: Vec<PaneAgent>,
}

/// A pane that held a catalog agent. `program` is the last spawn
/// command (a fallback when the catalog no longer has `name`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PaneAgent {
    pane: String,
    name: String,
    #[serde(default)]
    acp: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    program: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WindowFile {
    id: String,
    tree: Tree,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    note: String,
}

#[derive(Debug, Clone)]
struct Window {
    id: String,
    tree: Tree,
    note: String,
}

pub struct Session {
    root: PathBuf,
    name: String,
    next_id: u64,
    tty_cols: u16,
    tty_rows: u16,
    gap: u16,
    windows: Vec<Window>,
    focused: String,
    panes: HashMap<String, Arc<Pane>>,
    acp: HashMap<String, Arc<AcpChild>>,
    watch: HashMap<String, Arc<HttpWatch>>,
    /// pane id → catalog name for agent processes.
    names: HashMap<String, String>,
    /// pane id → last spawn program, for disk.
    programs: HashMap<String, String>,
    /// panes whose name came from a shell that launched a catalog agent.
    adopted: HashSet<String>,
    catalog: Agents,
}

/// The sessions the daemon owns. Named, on disk under a root.
pub struct Sessions {
    root: PathBuf,
    live: Mutex<HashMap<String, Arc<Mutex<Session>>>>,
}

impl Sessions {
    pub fn open(root: PathBuf) -> io::Result<Sessions> {
        fs::create_dir_all(&root)?;
        Ok(Sessions {
            root,
            live: Mutex::new(HashMap::new()),
        })
    }

    /// The current tiling config, re-read from disk so a config
    /// change applies to the next layout without a daemon restart.
    pub fn tiling(&self) -> Tiling {
        Tiling::load(&self.root)
    }

    /// Enumerate — name every session it owns.
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.root)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().join(FILE).is_file())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// Create a session — give it a name. A new session has one window
    /// with one pane.
    pub fn create(&self, name: &str) -> io::Result<Arc<Mutex<Session>>> {
        let dir = self.root.join(name);
        if dir.join(FILE).exists() {
            return Err(io::Error::other("a session by that name already exists"));
        }
        let file = FileState {
            next_id: 2,
            tty_cols: DEFAULT_COLS,
            tty_rows: DEFAULT_ROWS,
            windows: vec![WindowFile {
                id: "sh".to_string(),
                tree: Tree::Leaf {
                    id: "1".to_string(),
                },
                note: String::new(),
            }],
            focused: "1".to_string(),
            agents: Vec::new(),
        };
        fs::create_dir_all(&dir)?;
        persist(&dir, &file)?;
        let session = Arc::new(Mutex::new(Session {
            root: dir,
            name: name.to_string(),
            next_id: file.next_id,
            tty_cols: file.tty_cols,
            tty_rows: file.tty_rows,
            gap: self.tiling().gap,
            windows: vec![Window {
                id: "sh".to_string(),
                tree: Tree::Leaf {
                    id: "1".to_string(),
                },
                note: String::new(),
            }],
            focused: file.focused,
            panes: HashMap::new(),
            acp: HashMap::new(),
            watch: HashMap::new(),
            names: HashMap::new(),
            programs: HashMap::new(),
            adopted: HashSet::new(),
            catalog: Agents::load(&self.root),
        }));
        self.live
            .lock()
            .map_err(|_| io::Error::other("sessions busy"))?
            .insert(name.to_string(), session.clone());
        Ok(session)
    }

    /// Open a session by name. Reopens from disk and spawns each
    /// named agent pane again.
    pub fn get(&self, name: &str) -> io::Result<Arc<Mutex<Session>>> {
        if let Some(session) = self
            .live
            .lock()
            .map_err(|_| io::Error::other("sessions busy"))?
            .get(name)
            .cloned()
        {
            return Ok(session);
        }
        let dir = self.root.join(name);
        if !dir.join(FILE).exists() {
            return Err(io::Error::other("no such session"));
        }
        let file = load(&dir)?;
        let agents = file.agents;
        let mut session = Session {
            root: dir,
            name: name.to_string(),
            next_id: file.next_id,
            tty_cols: file.tty_cols,
            tty_rows: file.tty_rows,
            gap: self.tiling().gap,
            windows: file
                .windows
                .into_iter()
                .map(|w| Window {
                    id: w.id,
                    tree: w.tree,
                    note: w.note,
                })
                .collect(),
            focused: file.focused,
            panes: HashMap::new(),
            acp: HashMap::new(),
            watch: HashMap::new(),
            names: HashMap::new(),
            programs: HashMap::new(),
            adopted: HashSet::new(),
            catalog: Agents::load(&self.root),
        };
        session.resurrect(agents);
        let session = Arc::new(Mutex::new(session));
        self.live
            .lock()
            .map_err(|_| io::Error::other("sessions busy"))?
            .insert(name.to_string(), session.clone());
        Ok(session)
    }

    /// Rename a session: move its directory under the new name.
    pub fn rename(&self, session: &Arc<Mutex<Session>>, name: &str) -> io::Result<()> {
        let mut s = session
            .lock()
            .map_err(|_| io::Error::other("session busy"))?;
        let new_dir = self.root.join(name);
        if new_dir.join(FILE).exists() {
            return Err(io::Error::other("a session by that name already exists"));
        }
        let old_name = s.name.clone();
        fs::rename(&s.root, &new_dir)?;
        s.root = new_dir;
        s.name = name.to_string();
        let mut live = self
            .live
            .lock()
            .map_err(|_| io::Error::other("sessions busy"))?;
        live.remove(&old_name);
        live.insert(name.to_string(), session.clone());
        Ok(())
    }

    /// Destroy a session: kill its processes, remove its directory.
    pub fn destroy(&self, session: &Arc<Mutex<Session>>) -> io::Result<()> {
        let name = session
            .lock()
            .map_err(|_| io::Error::other("session busy"))?
            .name
            .clone();
        fs::remove_dir_all(self.root.join(&name))?;
        self.live
            .lock()
            .map_err(|_| io::Error::other("sessions busy"))?
            .remove(&name);
        Ok(())
    }

    /// Views of sessions the daemon already holds. Disk-only names
    /// stay unopened — they have no running agents to count.
    pub fn each_live_view<F>(&self, mut f: F)
    where
        F: FnMut(&str, SessionView),
    {
        let pairs: Vec<(String, Arc<Mutex<Session>>)> = {
            let Ok(live) = self.live.lock() else {
                return;
            };
            live.iter().map(|(n, s)| (n.clone(), s.clone())).collect()
        };
        for (name, session) in pairs {
            let Ok(mut s) = session.lock() else {
                continue;
            };
            f(&name, s.view());
        }
    }
}

impl Session {
    /// Read a session: its windows, their panes, each pane's geometry,
    /// and the focused pane.
    pub fn view(&mut self) -> SessionView {
        self.reap_dead_panes();
        self.adopt_agents();
        let mut windows = Vec::new();
        for window in &self.windows {
            let mut panes = Vec::new();
            let rects = rects_of(&window.tree, self.tty_cols, self.tty_rows, self.gap);
            collect_panes(&window.tree, &mut panes);
            panes.sort();
            let panes: Vec<PaneView> = panes
                .into_iter()
                .map(|id| {
                    let (x, y, cols, rows) = rects[&id];
                    PaneView {
                        pane: id.clone(),
                        x,
                        y,
                        cols,
                        rows,
                        name: self.names.get(&id).cloned(),
                        activity: self.pane_activity(&id),
                        state: self.pane_mark(&id),
                    }
                })
                .collect();
            let state = self.window_mark(&panes);
            windows.push(WindowView {
                window: window.id.clone(),
                panes,
                state,
                note: window.note.clone(),
            });
        }
        SessionView {
            windows,
            focused: self.focused.clone(),
        }
    }

    /// A shell that launched a catalog agent is named and watched the
    /// same way as prefix-a.
    fn adopt_agents(&mut self) {
        let ids: Vec<String> = self.panes.keys().cloned().collect();
        for id in ids {
            let Some(pid) = self.panes.get(&id).and_then(|p| p.pid()) else {
                continue;
            };
            match super::adopt::detect(pid, &self.catalog) {
                Some(hit) => {
                    self.names.insert(id.clone(), hit.name.clone());
                    self.adopted.insert(id.clone());
                    if let Some(url) = hit.watch {
                        if !self.watch.contains_key(&id) {
                            self.start_http(&id, &url, Some(&hit.name));
                        }
                    }
                }
                None if self.adopted.contains(&id) => {
                    self.names.remove(&id);
                    self.adopted.remove(&id);
                    if let Some(w) = self.watch.remove(&id) {
                        w.stop();
                    }
                }
                None => {}
            }
        }
    }

/// A new window in the session, with one pane. The name is the
    /// window. The new window becomes current: focus moves to its pane.
    pub fn add_window(&mut self, name: &str) -> io::Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(io::Error::other("a window needs a name"));
        }
        if name.contains('/') || name.contains('\0') {
            return Err(io::Error::other("a window name is a single word"));
        }
        if self.windows.iter().any(|w| w.id == name) {
            return Err(io::Error::other("a window by that name already exists"));
        }
        let pane = self.next_id.to_string();
        self.next_id += 1;
        self.windows.push(Window {
            id: name.to_string(),
            tree: Tree::Leaf { id: pane.clone() },
            note: String::new(),
        });
        self.focused = pane;
        self.persist()?;
        Ok(name.to_string())
    }

    /// The window under its new name. Panes stay. The name is the
    /// window.
    pub fn rename_window(&mut self, window: &str, name: &str) -> io::Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(io::Error::other("a window needs a name"));
        }
        if name.contains('/') || name.contains('\0') {
            return Err(io::Error::other("a window name is a single word"));
        }
        if window != name && self.windows.iter().any(|w| w.id == name) {
            return Err(io::Error::other("a window by that name already exists"));
        }
        let w = self
            .windows
            .iter_mut()
            .find(|w| w.id == window)
            .ok_or_else(|| io::Error::other("no such window"))?;
        w.id = name.to_string();
        self.persist()
    }

    /// The markdown blob stored with this window.
    pub fn set_note(&mut self, window: &str, note: &str) -> io::Result<()> {
        let w = self
            .windows
            .iter_mut()
            .find(|w| w.id == window)
            .ok_or_else(|| io::Error::other("no such window"))?;
        w.note = note.to_string();
        self.persist()
    }

    /// Move the focus into a window: its first pane becomes the
    /// focused pane, and the window becomes the current one.
    pub fn focus(&mut self, window_id: &str) -> io::Result<()> {
        let window = self
            .windows
            .iter()
            .find(|w| w.id == window_id)
            .ok_or_else(|| io::Error::other("no such window"))?;
        let mut panes = Vec::new();
        collect_panes(&window.tree, &mut panes);
        panes.sort();
        let pane = panes
            .first()
            .ok_or_else(|| io::Error::other("the window has no panes"))?;
        self.focused = pane.clone();
        self.persist()
    }

    /// Move the focus to a pane: the pane becomes the focused pane.
    pub fn focus_pane(&mut self, pane_id: &str) -> io::Result<()> {
        if !self.windows.iter().any(|w| {
            let mut panes = Vec::new();
            collect_panes(&w.tree, &mut panes);
            panes.contains(&pane_id.to_string())
        }) {
            return Err(io::Error::other("no such pane"));
        }
        self.focused = pane_id.to_string();
        self.persist()
    }

    fn pane_activity(&self, id: &str) -> Option<String> {
        self.watch
            .get(id)
            .and_then(|w| w.activity())
            .or_else(|| {
                let name = self.names.get(id)?;
                let home = self.catalog.by_name(name)?.door().inhibit()?.home.clone()?;
                let pid = self.panes.get(id)?.pid()?;
                super::inhibit::activity(pid, &home)
            })
    }

    fn pane_turning(&self, pane_id: &str, pid: u32) -> bool {
        let Some(name) = self.names.get(pane_id) else {
            return false;
        };
        let Some(spec) = self.catalog.by_name(name).map(|a| a.door()) else {
            return false;
        };
        let Some(inh) = spec.inhibit() else {
            return false;
        };
        super::inhibit::turning(pid, &inh.contains)
    }

    fn start_http(&mut self, pane_id: &str, url: &str, name: Option<&str>) {
        let spec = name
            .and_then(|n| self.catalog.by_name(n))
            .and_then(|a| a.door().http().cloned())
            .unwrap_or_default();
        self.watch
            .insert(pane_id.to_string(), HttpWatch::start(url, spec));
    }

    /// A process that has ended takes its pane with it. `exit` in a
    /// shell closes the pane; the last pane of a window takes the
    /// window. A pane that never got a process is left for spawn.
    fn reap_dead_panes(&mut self) {
        let mut dead = Vec::new();
        for (id, pane) in &self.panes {
            if !pane.alive() {
                dead.push(id.clone());
            }
        }
        for (id, child) in &self.acp {
            if !child.alive() {
                dead.push(id.clone());
            }
        }
        for id in dead {
            let _ = self.close_pane(&id);
        }
    }

    /// Close a pane: its process ends; the pane leaves the window, and
    /// the layout re-tiles. A pane that was the window's only pane
    /// takes the window with it. If the closed pane was focused, focus
    /// moves to the window's first pane (or the next window's).
    pub fn close_pane(&mut self, pane_id: &str) -> io::Result<()> {
        let idx = self
            .windows
            .iter()
            .position(|w| {
                let mut panes = Vec::new();
                collect_panes(&w.tree, &mut panes);
                panes.contains(&pane_id.to_string())
            })
            .ok_or_else(|| io::Error::other("no such pane"))?;
        if let Some(pane) = self.panes.remove(pane_id) {
            pane.hangup();
        }
        self.drop_process(pane_id);
        let tree = self.windows[idx].tree.clone();
        match remove_leaf(&tree, pane_id) {
            Some(tree) => self.windows[idx].tree = tree,
            None => {
                self.windows.remove(idx);
            }
        }
        self.refocus();
        self.relay_panes();
        self.persist()
    }

    /// Close a window: its panes' processes end; the window leaves the
    /// session. If the closed window was current, focus moves to
    /// another window's first pane.
    pub fn close_window(&mut self, window_id: &str) -> io::Result<()> {
        let idx = self
            .windows
            .iter()
            .position(|w| w.id == window_id)
            .ok_or_else(|| io::Error::other("no such window"))?;
        let mut panes = Vec::new();
        collect_panes(&self.windows[idx].tree, &mut panes);
        for pane_id in &panes {
            if let Some(pane) = self.panes.remove(pane_id) {
                pane.hangup();
            }
            self.drop_process(pane_id);
        }
        self.windows.remove(idx);
        self.refocus();
        self.persist()
    }

    /// After a close, if the focused pane is gone, focus the first
    /// pane of the first window; a windowless session focuses none.
    fn refocus(&mut self) {
        if self.windows.iter().any(|w| {
            let mut panes = Vec::new();
            collect_panes(&w.tree, &mut panes);
            panes.contains(&self.focused)
        }) {
            return;
        }
        self.focused = self
            .windows
            .first()
            .and_then(|w| {
                let mut panes = Vec::new();
                collect_panes(&w.tree, &mut panes);
                panes.sort();
                panes.first().cloned()
            })
            .unwrap_or_default();
    }

    /// Split a window: its focused pane becomes two panes, tiled.
    /// `rows` stacks them; otherwise they sit side by side.
    pub fn split(&mut self, window_id: &str, rows: bool) -> io::Result<()> {
        let idx = self
            .windows
            .iter()
            .position(|w| w.id == window_id)
            .ok_or_else(|| io::Error::other("no such window"))?;
        let tree = self.windows[idx].tree.clone();
        let rects = rects_of(&tree, self.tty_cols, self.tty_rows, self.gap);
        if !rects.contains_key(&self.focused) {
            return Err(io::Error::other("no such pane"));
        }
        let dir = if rows { Dir::Rows } else { Dir::Cols };
        let new_id = self.next_id.to_string();
        self.windows[idx].tree = split_into(&tree, &self.focused, dir, self.next_id);
        self.next_id += 1;
        self.focused = new_id;
        self.relay_panes();
        self.persist()
    }

    /// Resize the tty: the panes relay out; the processes are told.
    pub fn resize(&mut self, cols: u16, rows: u16, gap: u16) -> io::Result<()> {
        self.gap = gap;
        self.tty_cols = cols.max(2);
        self.tty_rows = rows.max(2);
        self.relay_panes();
        self.persist()
    }

    /// Tell every live PTY the size its tile now has.
    fn relay_panes(&self) {
        let rects = self.all_rects();
        for (id, pane) in self.panes.iter() {
            if let Some((_, _, cols, rows)) = rects.get(id) {
                let _ = pane.resize(*cols, *rows);
            }
        }
    }

    /// Spawn a process in a pane. PTY by default; `acp` holds stdio
    /// and speaks ACP. The new process replaces whatever is already
    /// on the pane — a new window always has a shell first.
    pub fn spawn(
        &mut self,
        pane_id: &str,
        program: &str,
        acp: bool,
        watch: Option<&str>,
        name: Option<&str>,
    ) -> io::Result<()> {
        if let Some(w) = self.watch.remove(pane_id) {
            w.stop();
        }
        if let Some(pane) = self.panes.remove(pane_id) {
            pane.terminate();
        }
        if let Some(child) = self.acp.remove(pane_id) {
            child.terminate();
        }
        self.names.remove(pane_id);
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            self.names.insert(pane_id.to_string(), name.to_string());
        }
        self.programs.insert(pane_id.to_string(), program.to_string());
        if acp {
            let child = AcpChild::spawn(program)?;
            self.acp.insert(pane_id.to_string(), child);
            let _ = self.persist();
            return Ok(());
        }
        let (_, _, cols, rows) = self
            .pane_geometry(pane_id)
            .ok_or_else(|| io::Error::other("no such pane"))?;
        let pane = Pane::spawn(program, cols, rows)?;
        self.panes.insert(pane_id.to_string(), pane);
        if let Some(url) = watch {
            self.start_http(pane_id, url, name);
        }
        self.persist()
    }

    fn drop_process(&mut self, pane_id: &str) {
        if let Some(w) = self.watch.remove(pane_id) {
            w.stop();
        }
        if let Some(child) = self.acp.remove(pane_id) {
            child.terminate();
        }
        self.names.remove(pane_id);
        self.programs.remove(pane_id);
        self.adopted.remove(pane_id);
    }

    fn agent_records(&self) -> Vec<PaneAgent> {
        let mut live = HashSet::new();
        for w in &self.windows {
            let mut panes = Vec::new();
            collect_panes(&w.tree, &mut panes);
            live.extend(panes);
        }
        self.names
            .iter()
            .filter(|(id, _)| live.contains(*id))
            .map(|(id, name)| PaneAgent {
                pane: id.clone(),
                name: name.clone(),
                acp: self.acp.contains_key(id),
                program: self.programs.get(id).cloned().unwrap_or_default(),
            })
            .collect()
    }

    /// Spawn each named agent again. Catalog wins (fresh HTTP ports).
    /// Unknown names use the stored program.
    fn resurrect(&mut self, records: Vec<PaneAgent>) {
        if records.is_empty() {
            return;
        }
        let catalog = self.catalog.clone();
        for rec in records {
            if rec.name.is_empty() {
                continue;
            }
            let result = if let Some(agent) = catalog.by_name(&rec.name) {
                if rec.acp {
                    match agent.acp_cmd() {
                        Some(cmd) => self.spawn(&rec.pane, cmd, true, None, Some(&rec.name)),
                        None => continue,
                    }
                } else {
                    let (program, watch) = agent.tui_spawn();
                    self.spawn(
                        &rec.pane,
                        &program,
                        false,
                        watch.as_deref(),
                        Some(&rec.name),
                    )
                }
            } else if rec.program.is_empty() {
                continue;
            } else {
                self.spawn(&rec.pane, &rec.program, rec.acp, None, Some(&rec.name))
            };
            if let Err(err) = result {
                eprintln!(
                    "anvil: could not restore agent {} on pane {}: {err}",
                    rec.name, rec.pane
                );
            }
        }
    }

    /// Write to a process. Keys go to the PTY or ACP composer.
    /// A prompt is a turn on the agent door: ACP `session/prompt`,
    /// or OpenCode's HTTP session — the same context as the TUI.
    pub fn write(&self, data: &str, pane: Option<&str>, prompt: bool) -> io::Result<()> {
        let pane_id = pane.unwrap_or(self.focused.as_str());
        if prompt {
            return self.prompt_pane(pane_id, data);
        }
        if let Some(acp) = self.acp.get(pane_id) {
            return acp.write_keys(data);
        }
        let pane = self
            .panes
            .get(pane_id)
            .cloned()
            .ok_or_else(|| io::Error::other("the focused pane has no process"))?;
        pane.write(data.as_bytes())
    }

    fn prompt_pane(&self, pane_id: &str, text: &str) -> io::Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        if let Some(acp) = self.acp.get(pane_id) {
            return acp.prompt(text);
        }
        if let Some(w) = self.watch.get(pane_id) {
            return w.prompt(text);
        }
        Err(io::Error::other("this pane has no agent door"))
    }

    /// Read a pane's grid. An ACP pane has no grid; it is alive while
    /// the child is.
    pub fn read_pane(&self, pane_id: &str) -> Grid {
        if let Some(acp) = self.acp.get(pane_id) {
            let (_, _, cols, rows) = self
                .pane_geometry(pane_id)
                .unwrap_or((0, 0, DEFAULT_COLS, DEFAULT_ROWS));
            return acp.grid(cols, rows);
        }
        let grid = self.panes.get(pane_id).cloned().map(|pane| pane.grid());
        if let Some(grid) = grid {
            return grid;
        }
        let (_, _, cols, rows) = self
            .pane_geometry(pane_id)
            .unwrap_or((0, 0, DEFAULT_COLS, DEFAULT_ROWS));
        Grid::blank(cols, rows)
    }

    /// End the processes the panes hold: `SIGHUP`, per the kernel.
    pub fn terminate(&self) {
        for pane in self.panes.values() {
            pane.hangup();
        }
        for acp in self.acp.values() {
            acp.hangup();
        }
        for w in self.watch.values() {
            w.stop();
        }
    }

    fn pane_geometry(&self, pane_id: &str) -> Option<(u16, u16, u16, u16)> {
        self.all_rects().get(pane_id).copied()
    }

    fn all_rects(&self) -> HashMap<String, (u16, u16, u16, u16)> {
        let mut rects = HashMap::new();
        for window in &self.windows {
            layout(&window.tree, self.tty_cols, self.tty_rows, 0, 0, self.gap, &mut rects);
        }
        rects
    }

    fn persist(&self) -> io::Result<()> {
        persist(
            &self.root,
            &FileState {
                next_id: self.next_id,
                tty_cols: self.tty_cols,
                tty_rows: self.tty_rows,
                windows: self
                    .windows
                    .iter()
                    .map(|w| WindowFile {
                        id: w.id.clone(),
                        tree: w.tree.clone(),
                        note: w.note.clone(),
                    })
                    .collect(),
                focused: self.focused.clone(),
                agents: self.agent_records(),
            },
        )
    }
}

fn collect_panes(tree: &Tree, out: &mut Vec<String>) {
    match tree {
        Tree::Leaf { id } => out.push(id.clone()),
        Tree::Split { a, b, .. } => {
            collect_panes(a, out);
            collect_panes(b, out);
        }
    }
}

/// Remove a leaf from the tree. A split collapses to its other side
/// when one side loses its last pane. Returns the new tree, or none
/// when the removed leaf was the tree.
fn remove_leaf(tree: &Tree, id: &str) -> Option<Tree> {
    match tree {
        Tree::Leaf { id: tid } => {
            if tid == id {
                None
            } else {
                Some(tree.clone())
            }
        }
        Tree::Split { dir, a, b } => match (remove_leaf(a, id), remove_leaf(b, id)) {
            (None, None) => None,
            (None, Some(b)) => Some(b),
            (Some(a), None) => Some(a),
            (Some(a), Some(b)) => Some(Tree::Split {
                dir: *dir,
                a: Box::new(a),
                b: Box::new(b),
            }),
        },
    }
}

fn layout(
    tree: &Tree,
    cols: u16,
    rows: u16,
    x: u16,
    y: u16,
    gap: u16,
    out: &mut HashMap<String, (u16, u16, u16, u16)>,
) {
    match tree {
        Tree::Leaf { id } => {
            out.insert(id.clone(), (x, y, cols, rows));
        }
        Tree::Split { dir, a, b } => match dir {
            Dir::Cols => {
                // The gap is the distance between the two tiles; the
                // canvas edge keeps no margin (the client's gutter
                // holds it).
                let span = cols.saturating_sub(gap).max(1);
                let half = span / 2;
                layout(a, half, rows, x, y, gap, out);
                layout(b, span - half, rows, x + half + gap, y, gap, out);
            }
            Dir::Rows => {
                let span = rows.saturating_sub(gap).max(1);
                let half = span / 2;
                layout(a, cols, half, x, y, gap, out);
                layout(b, cols, span - half, x, y + half + gap, gap, out);
            }
        },
    }
}

fn rects_of(
    tree: &Tree,
    cols: u16,
    rows: u16,
    gap: u16,
) -> HashMap<String, (u16, u16, u16, u16)> {
    let mut rects = HashMap::new();
    layout(tree, cols, rows, 0, 0, gap, &mut rects);
    rects
}

fn split_into(tree: &Tree, target: &str, dir: Dir, new_id: u64) -> Tree {
    match tree {
        Tree::Leaf { id } if id == target => Tree::Split {
            dir,
            a: Box::new(Tree::Leaf { id: id.clone() }),
            b: Box::new(Tree::Leaf {
                id: new_id.to_string(),
            }),
        },
        Tree::Leaf { .. } => tree.clone(),
        Tree::Split { dir: d, a, b } => Tree::Split {
            dir: *d,
            a: Box::new(split_into(a, target, dir, new_id)),
            b: Box::new(split_into(b, target, dir, new_id)),
        },
    }
}

fn load(dir: &Path) -> io::Result<FileState> {
    let bytes = fs::read(dir.join(FILE))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| io::Error::other(format!("cannot read {}: {err}", dir.join(FILE).display())))
}

fn persist(dir: &Path, state: &FileState) -> io::Result<()> {
    let bytes = serde_json::to_vec(state).map_err(|err| io::Error::other(err.to_string()))?;
    fs::write(dir.join(FILE), bytes)
}

impl Grid {
    fn blank(cols: u16, rows: u16) -> Grid {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let line = " ".repeat(cols as usize);
        let run = vec![crate::daemon::pane::Run {
            text: line.clone(),
            fg: None,
            fg_rgb: None,
            bg: None,
            bg_rgb: None,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
        }];
        Grid {
            cols,
            rows,
            cursor_col: 0,
            cursor_row: 0,
            lines: vec![line; rows as usize],
            runs: vec![run; rows as usize],
            alive: false,
            acp: false,
            mouse: false,
            kitty: 0,
            modify: false,
        }
    }
}

impl Session {
    fn pane_mark(&self, pane_id: &str) -> WindowState {
        if let Some(w) = self.watch.get(pane_id) {
            let s = w.state();
            if s != WindowState::Idle && s != WindowState::Dead {
                return s;
            }
        }
        if let Some(child) = self.acp.get(pane_id) {
            if !child.alive() {
                return WindowState::Dead;
            }
            return child.state();
        }
        if let Some(pane) = self.panes.get(pane_id) {
            if !pane.alive() {
                return WindowState::Dead;
            }
            if pane.pid().is_some_and(|pid| self.pane_turning(pane_id, pid)) {
                return WindowState::Turning;
            }
            WindowState::Idle
        } else {
            WindowState::Idle
        }
    }

    fn window_mark(&self, panes: &[PaneView]) -> WindowState {
        let mut any_live = false;
        let mut any_process = false;
        let mut turning = false;
        for p in panes {
            if let Some(w) = self.watch.get(&p.pane) {
                match w.state() {
                    WindowState::NeedsYou => return WindowState::NeedsYou,
                    WindowState::Turning => turning = true,
                    WindowState::Idle | WindowState::Dead => {}
                }
            }
            if let Some(child) = self.acp.get(&p.pane) {
                any_process = true;
                match child.state() {
                    WindowState::NeedsYou => return WindowState::NeedsYou,
                    WindowState::Turning => turning = true,
                    WindowState::Idle | WindowState::Dead => {}
                }
                if child.alive() {
                    any_live = true;
                }
            } else if let Some(pane) = self.panes.get(&p.pane) {
                any_process = true;
                if pane.alive() {
                    any_live = true;
                    if pane.pid().is_some_and(|pid| self.pane_turning(&p.pane, pid)) {
                        turning = true;
                    }
                }
            }
        }
        if turning {
            return WindowState::Turning;
        }
        if any_process && !any_live {
            WindowState::Dead
        } else {
            WindowState::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sessions() -> (tempfile::TempDir, Sessions) {
        let dir = tempfile::tempdir().unwrap();
        let sessions = Sessions::open(dir.path().join("root")).unwrap();
        (dir, sessions)
    }

    #[test]
    fn create_read_rename_destroy_round_trip() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        assert_eq!(sessions.list(), vec!["work".to_string()]);

        let work = sessions.get("work").unwrap();
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows.len(), 1);
        assert_eq!(view.windows[0].window, "sh");
        assert_eq!(view.windows[0].panes.len(), 1);
        assert_eq!(view.focused, "1");

        sessions.rename(&work, "deep-work").unwrap();
        assert_eq!(sessions.list(), vec!["deep-work".to_string()]);

        sessions.destroy(&work).unwrap();
        assert!(sessions.list().is_empty());
    }

    #[test]
    fn rename_into_existing_fails() {
        let (_dir, sessions) = sessions();
        sessions.create("a").unwrap();
        sessions.create("b").unwrap();
        let a = sessions.get("a").unwrap();
        let err = sessions.rename(&a, "b").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn split_down_stacks_the_panes() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("sh", true).unwrap();

        let view = work.lock().unwrap().view();
        assert_eq!(view.windows[0].panes.len(), 2);
        let a = &view.windows[0].panes[0];
        let b = &view.windows[0].panes[1];
        assert_eq!(a.x, 0);
        assert_eq!(b.x, 0);
        assert_eq!(a.cols, 80);
        assert_eq!(b.cols, 80);
        assert_eq!(a.y, 0);
        assert_eq!(b.y, a.rows + 1, "{a:?} {b:?}");
        assert_eq!(a.rows + b.rows, 23, "{a:?} {b:?}");
        assert_eq!(view.focused, "2");
    }

    #[test]
    fn split_tiles_and_persists_geometry() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("sh", false).unwrap();

        let view = work.lock().unwrap().view();
        assert_eq!(view.windows[0].panes.len(), 2);
        let a = &view.windows[0].panes[0];
        let b = &view.windows[0].panes[1];
        // The default gap is 1: exactly one cell between the two
        // tiles; the canvas edges keep no margin.
        assert_eq!(a.x, 0);
        assert_eq!(a.y, 0);
        assert_eq!(b.x, 40, "{a:?} {b:?}");
        assert_eq!(a.cols + b.cols, 79, "{a:?} {b:?}");
        assert_eq!(a.rows, 24);
        assert_eq!(b.rows, 24);
        assert_eq!(view.focused, "2");
    }

    #[test]
    fn reopen_from_disk_spawns_named_agents() {
        let (dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock()
            .unwrap()
            .spawn("1", "sh", false, None, Some("golem"))
            .unwrap();
        assert_eq!(
            work.lock().unwrap().view().windows[0].panes[0].name.as_deref(),
            Some("golem")
        );
        drop(work);

        let restarted = Sessions::open(dir.path().join("root")).unwrap();
        let work = restarted.get("work").unwrap();
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows[0].panes[0].name.as_deref(), Some("golem"));
        assert!(
            work.lock().unwrap().read_pane("1").alive,
            "the agent pane should be running again"
        );
    }

    #[test]
    fn reopen_from_disk_keeps_windows_and_panes() {
        let (dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("sh", false).unwrap();
        drop(work);

        // The daemon restarts: a fresh Sessions over the same root.
        let restarted = Sessions::open(dir.path().join("root")).unwrap();
        assert_eq!(restarted.list(), vec!["work".to_string()]);
        let work = restarted.get("work").unwrap();
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows[0].panes.len(), 2);
        assert_eq!(view.focused, "2");
    }

    #[test]
    fn resize_relays_out_the_panes() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("sh", false).unwrap();
        work.lock().unwrap().resize(100, 50, 1).unwrap();

        let view = work.lock().unwrap().view();
        // Gap 1: exactly one cell between the two tiles.
        assert_eq!(view.windows[0].panes[0].cols + view.windows[0].panes[1].cols, 99);
        assert_eq!(view.windows[0].panes[0].rows, 50);
    }

    #[test]
    fn rects_keep_the_gap_between_neighbors_only() {
        let tree = Tree::Split {
            dir: Dir::Cols,
            a: Box::new(Tree::Leaf { id: "1".into() }),
            b: Box::new(Tree::Leaf { id: "2".into() }),
        };
        // The gap is the distance between the two tiles, and nothing
        // more: the canvas edges keep no margin.
        let rects = rects_of(&tree, 20, 10, 2);
        assert_eq!(rects["1"], (0, 0, 9, 10));
        assert_eq!(rects["2"], (11, 0, 9, 10));

        // Gap 0 is the full-bleed layout from before gaps existed.
        let rects = rects_of(&tree, 20, 10, 0);
        assert_eq!(rects["1"], (0, 0, 10, 10));
        assert_eq!(rects["2"], (10, 0, 10, 10));
    }

    #[test]
    fn spawn_write_read_write_round_trip() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().spawn("1", "sh", false, None, None).unwrap();
        work.lock().unwrap().write("printf 'hello session'\n", None, false).unwrap();
        let mut grid = work.lock().unwrap().read_pane("1");
        for _ in 0..100 {
            if grid.lines.iter().any(|l| l.contains("hello session")) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            grid = work.lock().unwrap().read_pane("1");
        }
        assert!(
            grid.lines.iter().any(|l| l.contains("hello session")),
            "{grid:?}"
        );
    }

    #[test]
    fn pty_spawn_replaces_a_live_shell() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().spawn("1", "sh", false, None, None).unwrap();
        assert!(work.lock().unwrap().read_pane("1").alive);

        work.lock().unwrap().spawn("1", "sh", false, None, None).unwrap();
        assert!(work.lock().unwrap().read_pane("1").alive);
    }

    #[test]
    fn an_agent_spawn_names_the_pane() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock()
            .unwrap()
            .spawn("1", "sh", false, None, Some("oc"))
            .unwrap();
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows[0].panes[0].name.as_deref(), Some("oc"));
    }

    #[test]
    fn acp_spawn_replaces_a_live_shell() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().spawn("1", "sh", false, None, None).unwrap();
        assert!(work.lock().unwrap().read_pane("1").alive);
        assert!(!work.lock().unwrap().read_pane("1").acp);

        let (_keep, path) = crate::daemon::acp::tests::fake_agent();
        work.lock()
            .unwrap()
            .spawn("1", &format!("python3 {path}"), true, None, None)
            .unwrap();
        let grid = work.lock().unwrap().read_pane("1");
        assert!(grid.acp, "{grid:?}");
        assert!(grid.alive, "{grid:?}");
    }

    #[test]
    fn write_to_a_dead_process_is_an_error() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().spawn("1", "sh", false, None, None).unwrap();
        work.lock().unwrap().write("exit 0\n", None, false).unwrap();
        for _ in 0..100 {
            if !work.lock().unwrap().read_pane("1").alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let err = work.lock().unwrap().write("echo x\n", None, false).unwrap_err();
        assert!(err.to_string().contains("ended"), "{err}");
    }

    #[test]
    fn a_new_window_becomes_current_and_focus_moves() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("sh", false).unwrap();
        assert_eq!(work.lock().unwrap().view().focused, "2");

        work.lock().unwrap().add_window("plugin").unwrap();
        let view = work.lock().unwrap().view();
        // The new window is current: its pane is the focused one.
        assert_eq!(view.windows.len(), 2);
        assert_eq!(view.windows[1].window, "plugin");
        assert_eq!(view.focused, "3");
        assert!(view.windows[1].panes.iter().any(|p| p.pane == "3"));

        // Focus moves back to the first window.
        work.lock().unwrap().focus("sh").unwrap();
        assert_eq!(work.lock().unwrap().view().focused, "1");

        let err = work.lock().unwrap().focus("99").unwrap_err();
        assert!(err.to_string().contains("no such window"));

        let err = work.lock().unwrap().add_window("plugin").unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        let err = work.lock().unwrap().add_window("  ").unwrap_err();
        assert!(err.to_string().contains("needs a name"), "{err}");
    }

    #[test]
    fn a_window_note_survives_reopen_and_rename() {
        let (dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().set_note("sh", "- [ ] ship").unwrap();
        assert_eq!(work.lock().unwrap().view().windows[0].note, "- [ ] ship");
        work.lock().unwrap().rename_window("sh", "ui").unwrap();
        drop(work);

        let restarted = Sessions::open(dir.path().join("root")).unwrap();
        let view = restarted.get("work").unwrap().lock().unwrap().view();
        assert_eq!(view.windows[0].window, "ui");
        assert_eq!(view.windows[0].note, "- [ ] ship");
    }

    #[test]
    fn rename_window_keeps_the_panes() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().add_window("1").unwrap();
        work.lock().unwrap().rename_window("1", "plugin").unwrap();
        let view = work.lock().unwrap().view();
        assert!(view.windows.iter().any(|w| w.window == "plugin"));
        assert!(!view.windows.iter().any(|w| w.window == "1"));
        let err = work.lock().unwrap().rename_window("plugin", "sh").unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn a_dead_pane_closes() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("sh", false).unwrap();
        work.lock().unwrap().spawn("1", "sh", false, None, None).unwrap();
        work.lock().unwrap().spawn("2", "sh", false, None, None).unwrap();
        work.lock().unwrap().focus_pane("1").unwrap();
        work.lock().unwrap().write("exit 0\n", None, false).unwrap();
        for _ in 0..100 {
            if !work.lock().unwrap().read_pane("1").alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows[0].panes.len(), 1, "{view:?}");
        assert_eq!(view.windows[0].panes[0].pane, "2");
        assert_eq!(view.focused, "2");
    }

    #[test]
    fn exit_of_the_last_pane_closes_the_window() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().spawn("1", "sh", false, None, None).unwrap();
        work.lock().unwrap().write("exit 0\n", None, false).unwrap();
        for _ in 0..100 {
            if !work.lock().unwrap().read_pane("1").alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let view = work.lock().unwrap().view();
        assert!(view.windows.is_empty(), "{view:?}");
    }

    #[test]
    fn focus_pane_moves_the_focused_pane() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("sh", false).unwrap();
        assert_eq!(work.lock().unwrap().view().focused, "2");

        work.lock().unwrap().focus_pane("1").unwrap();
        assert_eq!(work.lock().unwrap().view().focused, "1");
        work.lock().unwrap().focus_pane("2").unwrap();
        assert_eq!(work.lock().unwrap().view().focused, "2");

        let err = work.lock().unwrap().focus_pane("99").unwrap_err();
        assert!(err.to_string().contains("no such pane"));
    }

    #[test]
    fn close_pane_collapses_the_split_and_moves_focus() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("sh", false).unwrap();
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows[0].panes.len(), 2);

        // Closing the original pane leaves the new one filling the window.
        work.lock().unwrap().close_pane("1").unwrap();
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows.len(), 1);
        assert_eq!(view.windows[0].panes.len(), 1);
        assert_eq!(view.windows[0].panes[0].pane, "2");
        assert_eq!(view.windows[0].panes[0].cols, 80);
        assert_eq!(view.windows[0].panes[0].rows, 24);
        // Focus moved to the remaining pane.
        assert_eq!(view.focused, "2");

        let err = work.lock().unwrap().close_pane("99").unwrap_err();
        assert!(err.to_string().contains("no such pane"));
    }

    #[test]
    fn close_pane_gives_the_remaining_process_the_window() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().spawn("1", "sh", false, None, None).unwrap();
        work.lock().unwrap().split("sh", false).unwrap();
        let half = work.lock().unwrap().read_pane("1");
        assert!(half.cols < 80, "split should shrink the original PTY: {half:?}");
        work.lock().unwrap().close_pane("2").unwrap();
        let full = work.lock().unwrap().read_pane("1");
        assert_eq!(full.cols, 80, "{full:?}");
        assert_eq!(full.rows, 24, "{full:?}");
    }

    #[test]
    fn close_the_only_pane_closes_the_window() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().add_window("plugin").unwrap();
        assert_eq!(work.lock().unwrap().view().windows.len(), 2);

        // The new window's only pane is focused; closing it closes
        // the window.
        work.lock().unwrap().close_pane("2").unwrap();
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows.len(), 1);
        assert_eq!(view.focused, "1");
    }

    #[test]
    fn close_window_ends_its_panes_and_moves_focus() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().spawn("1", "sh", false, None, None).unwrap();
        work.lock().unwrap().add_window("plugin").unwrap();
        // Current window is plugin (focused pane 2); window work has pane 1.
        work.lock().unwrap().focus("sh").unwrap();

        work.lock().unwrap().close_window("sh").unwrap();
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows.len(), 1);
        assert_eq!(view.windows[0].window, "plugin");
        // Pane 1's process ended.
        assert!(!view.windows[0].panes.iter().any(|p| p.pane == "1"));
        // Focus moved to the remaining window's pane.
        assert_eq!(view.focused, "2");

        let err = work.lock().unwrap().close_window("99").unwrap_err();
        assert!(err.to_string().contains("no such window"));
    }

    #[test]
    fn unknown_names_and_panes_are_errors() {
        let (_dir, sessions) = sessions();
        assert!(sessions.get("nope").is_err());
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        let err = work.lock().unwrap().split("9", false).unwrap_err();
        assert!(err.to_string().contains("no such window"), "{err}");
    }
}
