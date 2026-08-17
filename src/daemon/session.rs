//! The session store. A session is a named group of windows.
//! Kernel: "session — Named group of windows. Does not run." It lives
//! on disk, one directory per session, and reopens from it after a
//! daemon restart. Windows and panes carry the identifiers the daemon
//! issued when it made them.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::pane::{Grid, Pane};
use super::tiling::Tiling;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowView {
    pub window: String,
    pub panes: Vec<PaneView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneView {
    pub pane: String,
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
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
}

#[derive(Debug, Serialize, Deserialize)]
struct WindowFile {
    id: String,
    tree: Tree,
}

#[derive(Debug, Clone)]
struct Window {
    id: String,
    tree: Tree,
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
}

/// The sessions the daemon owns. Named, on disk under a root.
pub struct Sessions {
    root: PathBuf,
    tiling: Tiling,
    live: Mutex<HashMap<String, Arc<Mutex<Session>>>>,
}

impl Sessions {
    pub fn open(root: PathBuf) -> io::Result<Sessions> {
        fs::create_dir_all(&root)?;
        let tiling = Tiling::load(&root);
        Ok(Sessions {
            root,
            tiling,
            live: Mutex::new(HashMap::new()),
        })
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
                id: "1".to_string(),
                tree: Tree::Leaf {
                    id: "1".to_string(),
                },
            }],
            focused: "1".to_string(),
        };
        fs::create_dir_all(&dir)?;
        persist(&dir, &file)?;
        let session = Arc::new(Mutex::new(Session {
            root: dir,
            name: name.to_string(),
            next_id: file.next_id,
            tty_cols: file.tty_cols,
            tty_rows: file.tty_rows,
            gap: self.tiling.gap,
            windows: vec![Window {
                id: "1".to_string(),
                tree: Tree::Leaf {
                    id: "1".to_string(),
                },
            }],
            focused: file.focused,
            panes: HashMap::new(),
        }));
        self.live
            .lock()
            .map_err(|_| io::Error::other("sessions busy"))?
            .insert(name.to_string(), session.clone());
        Ok(session)
    }

    /// Open a session by name. Reopens from disk; the panes have no
    /// processes until one is spawned.
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
        let session = Arc::new(Mutex::new(Session {
            root: dir,
            name: name.to_string(),
            next_id: file.next_id,
            tty_cols: file.tty_cols,
            tty_rows: file.tty_rows,
            gap: self.tiling.gap,
            windows: file
                .windows
                .into_iter()
                .map(|w| Window { id: w.id, tree: w.tree })
                .collect(),
            focused: file.focused,
            panes: HashMap::new(),
        }));
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
}

impl Session {
    /// Read a session: its windows, their panes, each pane's geometry,
    /// and the focused pane.
    pub fn view(&self) -> SessionView {
        let mut windows = Vec::new();
        for window in &self.windows {
            let mut panes = Vec::new();
            let rects = rects_of(&window.tree, self.tty_cols, self.tty_rows, self.gap);
            collect_panes(&window.tree, &mut panes);
            panes.sort();
            let panes = panes
                .into_iter()
                .map(|id| {
                    let (x, y, cols, rows) = rects[&id];
                    PaneView {
                        pane: id,
                        x,
                        y,
                        cols,
                        rows,
                    }
                })
                .collect();
            windows.push(WindowView {
                window: window.id.clone(),
                panes,
            });
        }
        SessionView {
            windows,
            focused: self.focused.clone(),
        }
    }

    /// A new window in the session, with one pane.
    pub fn add_window(&mut self) -> io::Result<String> {
        let window = self.next_id.to_string();
        self.next_id += 1;
        let pane = self.next_id.to_string();
        self.next_id += 1;
        self.windows.push(Window {
            id: window.clone(),
            tree: Tree::Leaf { id: pane },
        });
        self.persist()?;
        Ok(window)
    }

    /// Split a window: its focused pane becomes two panes, tiled.
    pub fn split(&mut self, window_id: &str) -> io::Result<()> {
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
        let (_, _, cols, rows) = rects[&self.focused];
        let dir = if cols >= rows { Dir::Cols } else { Dir::Rows };
        self.windows[idx].tree = split_into(&tree, &self.focused, dir, self.next_id);
        self.next_id += 1;
        self.persist()
    }

    /// Resize the tty: the panes relay out; the processes are told.
    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        self.tty_cols = cols.max(2);
        self.tty_rows = rows.max(2);
        let rects = self.all_rects();
        for (id, pane) in self.panes.iter() {
            if let Some((_, _, cols, rows)) = rects.get(id) {
                pane.resize(*cols, *rows)?;
            }
        }
        self.persist()
    }

    /// Spawn a process in a pane. The daemon holds the master PTY; the
    /// process runs on the slave.
    pub fn spawn(&mut self, pane_id: &str, program: &str) -> io::Result<()> {
        let (_, _, cols, rows) = self
            .pane_geometry(pane_id)
            .ok_or_else(|| io::Error::other("no such pane"))?;
        if self.panes.contains_key(pane_id) {
            return Err(io::Error::other("the pane already has a process"));
        }
        let pane = Pane::spawn(program, cols, rows)?;
        self.panes.insert(pane_id.to_string(), pane);
        Ok(())
    }

    /// Write to the focused pane's process — the client's keys.
    pub fn write(&self, data: &str) -> io::Result<()> {
        let pane = self
            .panes
            .get(&self.focused)
            .cloned()
            .ok_or_else(|| io::Error::other("the focused pane has no process"))?;
        pane.write(data.as_bytes())
    }

    /// Read a pane's grid.
    pub fn read_pane(&self, pane_id: &str) -> Grid {        let grid = self
            .panes
            .get(pane_id)
            .cloned()
            .map(|pane| pane.grid());
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
                    })
                    .collect(),
                focused: self.focused.clone(),
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
            let cols = cols.saturating_sub(2 * gap).max(1);
            let rows = rows.saturating_sub(2 * gap).max(1);
            out.insert(
                id.clone(),
                (x.saturating_add(gap), y.saturating_add(gap), cols, rows),
            );
        }
        Tree::Split { dir, a, b } => match dir {
            Dir::Cols => {
                let half = cols / 2;
                layout(a, half, rows, x, y, gap, out);
                layout(b, cols - half, rows, x + half, y, gap, out);
            }
            Dir::Rows => {
                let half = rows / 2;
                layout(a, cols, half, x, y, gap, out);
                layout(b, cols, rows - half, x, y + half, gap, out);
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
    fn split_tiles_and_persists_geometry() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("1").unwrap();

        let view = work.lock().unwrap().view();
        assert_eq!(view.windows[0].panes.len(), 2);
        let a = &view.windows[0].panes[0];
        let b = &view.windows[0].panes[1];
        // The default gap is 1: each pane keeps a margin from its
        // neighbors and the canvas edge.
        assert_eq!(a.x, 1);
        assert_eq!(a.y, 1);
        assert_eq!(b.x, 41, "{a:?} {b:?}");
        assert_eq!(a.cols + b.cols, 76, "{a:?} {b:?}");
        assert_eq!(a.rows, 22);
        assert_eq!(b.rows, 22);
    }

    #[test]
    fn reopen_from_disk_keeps_windows_and_panes() {
        let (dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("1").unwrap();
        drop(work);

        // The daemon restarts: a fresh Sessions over the same root.
        let restarted = Sessions::open(dir.path().join("root")).unwrap();
        assert_eq!(restarted.list(), vec!["work".to_string()]);
        let work = restarted.get("work").unwrap();
        let view = work.lock().unwrap().view();
        assert_eq!(view.windows[0].panes.len(), 2);
        assert_eq!(view.focused, "1");
    }

    #[test]
    fn resize_relays_out_the_panes() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().split("1").unwrap();
        work.lock().unwrap().resize(100, 50).unwrap();

        let view = work.lock().unwrap().view();
        // Gap 1 shrinks each pane by 2 cols and 2 rows.
        assert_eq!(view.windows[0].panes[0].cols + view.windows[0].panes[1].cols, 96);
        assert_eq!(view.windows[0].panes[0].rows, 48);
    }

    #[test]
    fn rects_keep_the_gap_from_neighbors_and_canvas() {
        let tree = Tree::Split {
            dir: Dir::Cols,
            a: Box::new(Tree::Leaf { id: "1".into() }),
            b: Box::new(Tree::Leaf { id: "2".into() }),
        };
        let rects = rects_of(&tree, 20, 10, 2);
        // A margin of 2 on every side: 2 from the canvas edge, 4
        // between the two panes.
        assert_eq!(rects["1"], (2, 2, 6, 6));
        assert_eq!(rects["2"], (12, 2, 6, 6));

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
        work.lock().unwrap().spawn("1", "sh").unwrap();
        work.lock().unwrap().write("printf 'hello session'\n").unwrap();
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
    fn write_to_a_dead_process_is_an_error() {
        let (_dir, sessions) = sessions();
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        work.lock().unwrap().spawn("1", "sh").unwrap();
        work.lock().unwrap().write("exit 0\n").unwrap();
        for _ in 0..100 {
            if !work.lock().unwrap().read_pane("1").alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let err = work.lock().unwrap().write("echo x\n").unwrap_err();
        assert!(err.to_string().contains("ended"), "{err}");
    }

    #[test]
    fn unknown_names_and_panes_are_errors() {
        let (_dir, sessions) = sessions();
        assert!(sessions.get("nope").is_err());
        sessions.create("work").unwrap();
        let work = sessions.get("work").unwrap();
        let err = work.lock().unwrap().split("9").unwrap_err();
        assert!(err.to_string().contains("no such window"), "{err}");
    }
}
