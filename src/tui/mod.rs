//! The client: views a session and sends keys. Immediate-mode chrome
//! over the daemon's character grids. The palette is the opencode
//! builtin theme (opaline) — this module names semantic tokens only,
//! never colors of its own.

pub mod agents;
pub mod clip;
pub mod cwd;
pub mod focus;
pub mod keymap;
pub mod notes;
pub mod sat;
pub mod select;
pub mod sessions;
pub mod side;

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use opaline::Theme;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui_which_key::WhichKey;

use crate::daemon::acp::WindowState;
use crate::daemon::pane::Grid;
use crate::daemon::session::SessionView;
use crate::proto::{Reply, Request, Value};

use agents::{unique_name, Agent, Agents, Seat};
use keymap::{build_which_key_state, Action, AppWhichKey, Scope};
use notes::Notes;

/// The opencode palette, shipped with anvil and loaded through
/// opaline's public loader — opaline itself stays untouched.
const THEME_TOML: &str = include_str!("../../themes/opencode.toml");

// The chrome geometry.
const RAIL_COLS: u16 = 3; // mark column
const RAIL_MIN: u16 = 80; // rail shows from this width on
const SIDE_MIN: u16 = 80; // open sidebar on the same tty the rail does
const PAD: u16 = 2; // header chip and footer text, not the tiles
const GAP: u16 = 1; // between the sidebar and the content
const HEADER_LINES: u16 = 1; // session + host
const STATUS_LINES: u16 = 1; // footer shortcuts
const CHROME_ROWS: u16 = HEADER_LINES + STATUS_LINES;
const MARK_IDLE: &str = "◇";
const MARK_DEAD: &str = "◇";
const MARK_NEED: &str = "◆";
const DOT_FRAMES: &[&str] = &["⋅", ":", "⸬", "⁙"];

/// How the left chrome is drawn for this tty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Hidden,
    Rail,
    Open,
}

fn side(term_w: u16, open: bool) -> Side {
    if open && term_w >= SIDE_MIN {
        Side::Open
    } else if term_w >= RAIL_MIN {
        Side::Rail
    } else {
        Side::Hidden
    }
}

fn side_width(side: Side, cols: u16) -> u16 {
    match side {
        Side::Hidden => 0,
        Side::Rail => RAIL_COLS,
        Side::Open => cols,
    }
}

/// A direction of pane focus movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusDir {
    Left,
    Right,
    Up,
    Down,
}

/// The pane canvas the session tty occupies: the terminal minus the
/// chrome. The rail shows from `RAIL_MIN`; the open sidebar from
/// `SIDE_MIN`.
fn canvas(term_w: u16, term_h: u16, open: bool, cols: u16) -> (u16, u16) {
    let chrome = match side(term_w, open) {
        Side::Hidden => 0,
        other => side_width(other, cols) + GAP,
    };
    (
        term_w.saturating_sub(chrome),
        term_h.saturating_sub(CHROME_ROWS),
    )
}

fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "localhost".into())
}

/// One client connection: views a session, sends keys, keeps the
/// panes' grids it last read.
pub struct Client {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
    theme: Theme,
    shell: String,
    which_key: AppWhichKey,
    sessions: Vec<String>,
    attached: Option<String>,
    view: Option<SessionView>,
    grids: HashMap<String, Grid>,
    detached: bool,
    sidebar: side::Prefs,
    drag: Option<Drag>,
    tty: (u16, u16),
    /// Draft name: a window or the session.
    naming: Option<Naming>,
    tick: u64,
    last_error: Option<String>,
    catalog: Agents,
    picking: Option<usize>,
    /// After picking an agent that has both a native TUI and ACP.
    seat_pick: Option<(Agent, usize)>,
    /// Directory to start the agent in, after seat.
    cwd_pick: Option<CwdPick>,
    places: cwd::Places,
    sessions_pick: Option<usize>,
    session_rows: Vec<sessions::Row>,
    host: String,
    sat: crate::daemon::sat::Snap,
    prompting: Option<String>,
    notes: Option<Notes>,
    selection: Option<select::Selection>,
    toast: Option<Toast>,
    recency: HashMap<String, side::Recency>,
    seen_state: HashMap<String, WindowState>,
    term_focused: bool,
}

enum Drag {
    Width,
    Split,
}

struct Toast {
    message: String,
    until: Instant,
}

/// What the name draft is for.
enum Naming {
    Window(String),
    Session(String),
}

struct CwdPick {
    agent: Agent,
    seat: Seat,
    draft: String,
    sel: usize,
}

impl Client {
    /// Connect to the daemon at `sock` and take the first session,
    /// creating one if the daemon owns none.
    pub fn connect(sock: &Path) -> io::Result<Client> {
        let stream = UnixStream::connect(sock)?;
        let reader = BufReader::new(stream.try_clone()?);
        let theme = opaline::loader::load_from_str(THEME_TOML, None)
            .expect("the embedded opencode theme is valid");
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut client = Client {
            stream,
            reader,
            next_id: 0,
            theme,
            shell,
            which_key: build_which_key_state(),
            sessions: Vec::new(),
            attached: None,
            view: None,
            grids: HashMap::new(),
            detached: false,
            sidebar: side::Prefs::load(&agents::default_root()),
            drag: None,
            tty: (80, 24),
            naming: None,
            tick: 0,
            last_error: None,
            catalog: Agents::load(&agents::default_root()),
            picking: None,
            seat_pick: None,
            cwd_pick: None,
            places: cwd::Places::load(&agents::default_root()),
            sessions_pick: None,
            session_rows: Vec::new(),
            host: host_name(),
            sat: crate::daemon::sat::Snap::load(&agents::default_root()),
            prompting: None,
            notes: None,
            selection: None,
            toast: None,
            recency: HashMap::new(),
            seen_state: HashMap::new(),
            term_focused: true,
        };
        client.attach_first()?;
        Ok(client)
    }

    fn request(&mut self, request: Request) -> io::Result<Reply> {
        self.next_id += 1;
        let id = self.next_id.to_string();
        let request = request.with_id(&id);
        let mut line = serde_json::to_string(&request).map_err(io::Error::other)?;
        line.push('\n');
        self.stream.write_all(line.as_bytes())?;
        loop {
            let mut reply = String::new();
            self.reader.read_line(&mut reply)?;
            let reply: Reply = match serde_json::from_str(&reply) {
                Ok(reply) => reply,
                Err(err) => {
                    eprintln!(
                        "anvil client: bad reply (first 400 chars): {:?}",
                        &reply[..reply.len().min(400)]
                    );
                    return Err(io::Error::other(err));
                }
            };
            if reply.id == id {
                return Ok(reply);
            }
        }
    }

    fn call(&mut self, op: Request) -> io::Result<Value> {
        let reply = self.request(op)?;
        if reply.ok {
            reply
                .value
                .ok_or_else(|| io::Error::other("the daemon replied without a value"))
        } else {
            Err(io::Error::other(
                reply.error.unwrap_or_else(|| "the daemon refused".into()),
            ))
        }
    }

    fn enumerate(&mut self) -> io::Result<Vec<String>> {
        match self.call(Request::Enumerate { id: String::new() })? {
            Value::Sessions { sessions, .. } => {
                self.sessions = sessions.clone();
                Ok(sessions)
            }
            _ => Err(io::Error::other("enumerate replied with the wrong shape")),
        }
    }

    fn create(&mut self, name: &str) -> io::Result<()> {
        self.call(Request::Create {
            id: String::new(),
            session: name.into(),
            window: None,
        })?;
        Ok(())
    }

    fn attach(&mut self, name: &str) -> io::Result<()> {
        self.call(Request::Attach {
            id: String::new(),
            session: name.into(),
        })?;
        self.attached = Some(name.into());
        Ok(())
    }

    fn add_window(&mut self, name: &str) -> io::Result<()> {
        let session = self
            .attached
            .clone()
            .ok_or_else(|| io::Error::other("no session"))?;
        self.call(Request::Create {
            id: String::new(),
            session,
            window: Some(name.into()),
        })?;
        Ok(())
    }

    fn rename_window(&mut self, name: &str) -> io::Result<()> {
        let session = self
            .attached
            .clone()
            .ok_or_else(|| io::Error::other("no session"))?;
        let window = self
            .focused_window()
            .ok_or_else(|| io::Error::other("no window"))?;
        self.call(Request::Rename {
            id: String::new(),
            session,
            name: name.into(),
            window: Some(window),
            note: None,
        })?;
        Ok(())
    }

    fn rename_session(&mut self, name: &str) -> io::Result<()> {
        let session = self
            .attached
            .clone()
            .ok_or_else(|| io::Error::other("no session"))?;
        self.call(Request::Rename {
            id: String::new(),
            session,
            name: name.into(),
            window: None,
            note: None,
        })?;
        self.attached = Some(name.into());
        self.enumerate()?;
        Ok(())
    }

    fn save_note(&mut self, window: &str, note: &str) -> io::Result<()> {
        let session = self
            .attached
            .clone()
            .ok_or_else(|| io::Error::other("no session"))?;
        self.call(Request::Rename {
            id: String::new(),
            session,
            name: window.into(),
            window: Some(window.into()),
            note: Some(note.into()),
        })?;
        self.refresh()?;
        let stored = self
            .view
            .as_ref()
            .and_then(|v| v.windows.iter().find(|w| w.window == window))
            .map(|w| w.note.as_str())
            .unwrap_or("");
        if stored != note {
            return Err(io::Error::other(
                "the daemon did not keep the note; it is an older binary. anvil --restart",
            ));
        }
        Ok(())
    }

    fn split(&mut self, window: &str, rows: bool) -> io::Result<()> {
        self.call(Request::Split {
            id: String::new(),
            window: window.into(),
            rows,
        })?;
        Ok(())
    }

    fn read_view(&mut self) -> io::Result<SessionView> {
        match self.call(Request::Read {
            id: String::new(),
            session: self.attached.clone(),
            pane: None,
        })? {
            Value::View(view) => Ok(view),
            _ => Err(io::Error::other("read replied with the wrong shape")),
        }
    }

    fn read_pane(&mut self, pane: &str) -> io::Result<Grid> {
        match self.call(Request::Read {
            id: String::new(),
            session: None,
            pane: Some(pane.into()),
        })? {
            Value::Grid(grid) => Ok(grid),
            _ => Err(io::Error::other("read replied with the wrong shape")),
        }
    }

    fn spawn(&mut self, pane: &str) -> io::Result<()> {
        self.call(Request::Spawn {
            id: String::new(),
            pane: pane.into(),
            program: self.shell.clone(),
            acp: false,
            watch: None,
            name: None,
            cwd: None,
        })?;
        Ok(())
    }

    fn spawn_tui(
        &mut self,
        pane: &str,
        program: &str,
        watch: Option<String>,
        name: &str,
        cwd: Option<String>,
    ) -> io::Result<()> {
        self.call(Request::Spawn {
            id: String::new(),
            pane: pane.into(),
            program: program.into(),
            acp: false,
            watch,
            name: Some(name.into()),
            cwd,
        })?;
        Ok(())
    }

    fn window_names(&self) -> Vec<String> {
        self.view
            .as_ref()
            .map(|v| v.windows.iter().map(|w| w.window.clone()).collect())
            .unwrap_or_default()
    }

    /// A new window running the agent. Native is their TUI on a PTY.
    /// Anvil is the prompt/response viewer over ACP stdio.
    fn launch_agent(&mut self, agent: &Agent, seat: Seat, cwd: &str) -> io::Result<()> {
        let cwd = cwd::normalize(cwd);
        if !cwd::is_dir(&cwd) {
            self.last_error = Some(format!("no such directory: {cwd}"));
            return Ok(());
        }
        self.places.remember(&cwd, &agents::default_root());
        let name = unique_name(&agent.name, &self.window_names());
        self.add_window(&name)?;
        self.refresh()?;
        let pane = self.view.as_ref().map(|v| v.focused.clone());
        let Some(pane) = pane else {
            return Ok(());
        };
        let result = match seat {
            Seat::Anvil => match agent.acp_cmd() {
                Some(program) => self
                    .call(Request::Spawn {
                        id: String::new(),
                        pane: pane.clone(),
                        program: program.to_string(),
                        acp: true,
                        watch: None,
                        name: Some(agent.name.clone()),
                        cwd: Some(cwd),
                    })
                    .map(|_| ()),
                None => Err(io::Error::other(format!(
                    "{} has no ACP program",
                    agent.name
                ))),
            },
            Seat::Native => {
                let (program, watch) = agent.tui_spawn();
                self.spawn_tui(&pane, &program, watch, &agent.name, Some(cwd))
            }
        };
        match result {
            Ok(()) => self.last_error = None,
            Err(err) => self.last_error = Some(err.to_string()),
        }
        self.refresh()
    }

    fn pane_cwd(&self) -> Option<String> {
        let view = self.view.as_ref()?;
        view.windows
            .iter()
            .flat_map(|w| w.panes.iter())
            .find(|p| p.pane == view.focused)
            .and_then(|p| p.cwd.clone())
            .or_else(|| self.places.recent.first().cloned())
            .or_else(|| cwd::home().to_str().map(str::to_string))
    }

    fn begin_cwd_pick(&mut self, agent: Agent, seat: Seat) {
        let here = self
            .pane_cwd()
            .unwrap_or_else(|| cwd::home().to_string_lossy().into_owned());
        self.cwd_pick = Some(CwdPick {
            agent,
            seat,
            draft: here,
            sel: 0,
        });
    }

    fn cwd_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> io::Result<()> {
        use ratatui::crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc => {
                self.cwd_pick = None;
                Ok(())
            }
            KeyCode::Enter => {
                let Some(pick) = self.cwd_pick.take() else {
                    return Ok(());
                };
                let rows = self.cwd_rows(&pick);
                let path = rows
                    .get(pick.sel)
                    .map(|r| r.path.clone())
                    .filter(|_| !rows.is_empty())
                    .unwrap_or_else(|| cwd::normalize(&pick.draft));
                self.launch_agent(&pick.agent, pick.seat, &path)
            }
            KeyCode::Down | KeyCode::Up => {
                let n = self
                    .cwd_pick
                    .as_ref()
                    .map(|p| self.cwd_rows(p).len())
                    .unwrap_or(0);
                if let Some(pick) = self.cwd_pick.as_mut() {
                    if n > 0 {
                        pick.sel = step_pick(pick.sel, n, matches!(key.code, KeyCode::Down));
                    }
                }
                Ok(())
            }
            KeyCode::Tab => {
                let complete = self
                    .cwd_pick
                    .as_ref()
                    .and_then(|p| self.cwd_rows(p).get(p.sel).map(|r| r.path.clone()));
                if let (Some(path), Some(pick)) = (complete, self.cwd_pick.as_mut()) {
                    pick.draft = path;
                }
                Ok(())
            }
            KeyCode::Backspace => {
                if let Some(pick) = self.cwd_pick.as_mut() {
                    pick.draft.pop();
                    pick.sel = 0;
                }
                Ok(())
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .contains(ratatui::crossterm::event::KeyModifiers::CONTROL) =>
            {
                if let Some(pick) = self.cwd_pick.as_mut() {
                    pick.draft.push(c);
                    pick.sel = 0;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn cwd_rows(&self, pick: &CwdPick) -> Vec<cwd::Row> {
        self.places
            .rows_for(self.pane_cwd().as_deref(), &pick.draft)
    }

    /// Native when they have a TUI; otherwise anvil.
    fn launch_preferred(&mut self, agent: &Agent) -> io::Result<()> {
        let seat = agent.seats().into_iter().next().unwrap_or(Seat::Native);
        self.begin_cwd_pick(agent.clone(), seat);
        Ok(())
    }

    /// One seat launches now. Two seats open the native / anvil list.
    fn confirm_agent(&mut self, agent: Agent) -> io::Result<()> {
        let seats = agent.seats();
        if seats.len() <= 1 {
            return self.launch_preferred(&agent);
        }
        self.picking = None;
        self.seat_pick = Some((agent, 0));
        Ok(())
    }

    fn cancel_seat_pick(&mut self) {
        if let Some((agent, _)) = self.seat_pick.take() {
            self.picking = Some(
                self.catalog
                    .agents
                    .iter()
                    .position(|a| a.name == agent.name)
                    .unwrap_or(0),
            );
        }
    }

    /// A new window running a shell.
    fn launch_terminal(&mut self) -> io::Result<()> {
        let name = unique_name("sh", &self.window_names());
        self.add_window(&name)?;
        self.refresh()
    }

    fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        self.call(Request::Resize {
            id: String::new(),
            cols,
            rows,
        })?;
        Ok(())
    }

    fn write(&mut self, data: &str) -> io::Result<()> {
        self.call(Request::Write {
            id: String::new(),
            data: data.into(),
            pane: None,
            prompt: false,
        })?;
        Ok(())
    }

    /// Attach to the first session, creating one when the daemon owns
    /// none. The session tty fits the canvas; every pane gets a
    /// process.
    pub fn attach_first(&mut self) -> io::Result<()> {
        let sessions = self.enumerate()?;
        if let Some(name) = sessions.first().cloned() {
            self.attach(&name)?;
            return self.refresh();
        }
        for guess in ["1", "main"] {
            if self.attach(guess).is_ok() {
                let _ = self.enumerate();
                return self.refresh();
            }
        }
        let name = unused_session_name(&[]);
        match self.create(&name) {
            Ok(()) => {}
            Err(err) if err.to_string().contains("already exists") => {}
            Err(err) => return Err(err),
        }
        self.enumerate()?;
        let name = self.sessions.first().cloned().unwrap_or(name);
        self.attach(&name)?;
        self.refresh()
    }

    /// A new session, and attach to it.
    pub fn new_session(&mut self) -> io::Result<()> {
        self.enumerate()?;
        let mut name = unused_session_name(&self.sessions);
        loop {
            match self.create(&name) {
                Ok(()) => break,
                Err(err) if err.to_string().contains("already exists") => {
                    self.enumerate()?;
                    name = unused_session_name(&self.sessions);
                }
                Err(err) => return Err(err),
            }
        }
        self.enumerate()?;
        self.attach(&name)?;
        let (w, h) = self.tty;
        self.resize_tty(w, h)?;
        self.refresh()?;
        // Name it now — a bare number is not a place you can recognize.
        self.begin_naming(Naming::Session(String::new()));
        Ok(())
    }

    fn begin_naming(&mut self, naming: Naming) {
        self.last_error = None;
        self.naming = Some(naming);
    }

    /// Attach to session `n` (1-based), wrapping around.
    pub fn switch_session(&mut self, n: u8) -> io::Result<()> {
        let sessions = self.enumerate()?;
        if sessions.is_empty() {
            return Ok(());
        }
        let idx = (n as usize - 1) % sessions.len();
        let name = sessions[idx].clone();
        self.attach(&name)?;
        let (w, h) = self.tty;
        self.resize_tty(w, h)?;
        self.refresh()
    }

    fn read_session_view(&mut self, name: &str) -> io::Result<SessionView> {
        match self.call(Request::Read {
            id: String::new(),
            session: Some(name.into()),
            pane: None,
        })? {
            Value::View(view) => Ok(view),
            _ => Err(io::Error::other("read replied with the wrong shape")),
        }
    }

    fn load_session_rows(&mut self) -> io::Result<()> {
        let names = self.enumerate()?;
        let mut rows = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let view = self.read_session_view(name)?;
            rows.push(sessions::row(i + 1, name, &view));
        }
        self.session_rows = rows;
        Ok(())
    }

    fn destroy_picked_session(&mut self) -> io::Result<()> {
        let Some(idx) = self.sessions_pick else {
            return Ok(());
        };
        let Some(name) = self.sessions.get(idx).cloned() else {
            return Ok(());
        };
        if self.sessions.len() <= 1 {
            return Ok(());
        }
        self.call(Request::Destroy {
            id: String::new(),
            session: name.clone(),
        })?;
        let was_current = self.attached.as_deref() == Some(name.as_str());
        self.enumerate()?;
        if was_current {
            if let Some(next) = self.sessions.first().cloned() {
                self.attach(&next)?;
                let (w, h) = self.tty;
                self.resize_tty(w, h)?;
                self.refresh()?;
            }
        }
        self.load_session_rows()?;
        let n = self.session_rows.len();
        if n == 0 {
            self.sessions_pick = None;
        } else {
            self.sessions_pick = Some(idx.min(n - 1));
        }
        Ok(())
    }

    /// The tty changed size: the session relays out to the canvas.
    pub fn resize_tty(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        self.tty = (cols, rows);
        let (cols, rows) = canvas(cols, rows, self.sidebar.open, self.sidebar_cols());
        self.resize(cols.max(2), rows.max(2))
    }

    fn sidebar_cols(&self) -> u16 {
        self.sidebar.clamp_cols(self.tty.0)
    }

    fn save_sidebar(&self) {
        self.sidebar.save(&agents::default_root());
    }

    /// Read the view and every visible pane's grid; spawn a process on
    /// the panes that have none.
    pub fn refresh(&mut self) -> io::Result<()> {
        let view = self.read_view()?;
        let mut ids = Vec::new();
        for window in &view.windows {
            for pane in &window.panes {
                ids.push(pane.pane.clone());
            }
        }
        for id in ids {
            let grid = self.read_pane(&id)?;
            // No process yet (a fresh split): start a shell.
            // A process that already ended is reaped by the daemon.
            if !grid.alive && !grid.acp {
                let _ = self.spawn(&id);
            }
            self.grids.insert(id, grid);
        }
        self.note_agents(&view);
        self.view = Some(view);
        self.sat = crate::daemon::sat::Snap::load(&agents::default_root());
        Ok(())
    }

    /// Recency for the agent list, and a bell when a turn ends while
    /// the operator is not looking at that pane.
    fn note_agents(&mut self, view: &SessionView) {
        let now = Instant::now();
        let focused = view.focused.as_str();
        let mut bells: Vec<String> = Vec::new();
        for window in &view.windows {
            for pane in &window.panes {
                if pane.name.is_none() {
                    continue;
                }
                let id = pane.pane.as_str();
                let was_run = self.seen_state.get(id).copied().is_some_and(side::running);
                let is_run = side::running(pane.state);
                let slot = self.recency.entry(id.to_string()).or_default();
                if is_run {
                    slot.active = true;
                    if !was_run {
                        slot.last = Some(now);
                    }
                } else {
                    if was_run {
                        slot.last = Some(now);
                        bells.push(id.to_string());
                    }
                    slot.active = false;
                }
                self.seen_state.insert(id.to_string(), pane.state);
            }
        }
        let app = focus::app_is_active(self.term_focused);
        for pane in bells {
            if focus::should_bell(app, focused == pane) {
                focus::bell();
            }
        }
    }

    /// One key. The prefix (`ctrl-b`) arms the chrome; the keys that
    /// follow are actions. All other keys go to the focused pane's
    /// process. `esc` detaches.
    pub fn key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> io::Result<()> {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return Ok(());
        }
        if let Ok(dbg) = std::env::var("ANVIL_KEY_DEBUG") {
            let _ = (|| -> std::io::Result<()> {
                use std::io::Write as _;
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&dbg)?;
                writeln!(
                    f,
                    "key: code={:?} mods={:?} active={}",
                    key.code, key.modifiers, self.which_key.active
                )
            })();
        }
        if self.cwd_pick.is_some() {
            return self.cwd_key(key);
        }
        if let Some(idx) = self.sessions_pick {
            let n = self.sessions.len();
            match key.code {
                KeyCode::Esc => {
                    self.sessions_pick = None;
                    return Ok(());
                }
                KeyCode::Enter => {
                    self.sessions_pick = None;
                    if n > 0 {
                        return self.switch_session((idx as u8) + 1);
                    }
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.sessions_pick = Some(step_pick(idx, n, true));
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.sessions_pick = Some(step_pick(idx, n, false));
                    return Ok(());
                }
                KeyCode::Char('n') => {
                    self.sessions_pick = None;
                    return self.new_session();
                }
                KeyCode::Char('$') => {
                    self.sessions_pick = None;
                    return self.dispatch(Action::RenameSession);
                }
                KeyCode::Char('x') => {
                    return self.destroy_picked_session();
                }
                KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                    self.sessions_pick = None;
                    return self.switch_session(c as u8 - b'0');
                }
                _ => return Ok(()),
            }
        }
        if let Some((agent, idx)) = self.seat_pick.clone() {
            let seats = agent.seats();
            let n = seats.len();
            match key.code {
                KeyCode::Esc => {
                    self.cancel_seat_pick();
                    return Ok(());
                }
                KeyCode::Enter => {
                    self.seat_pick = None;
                    if let Some(seat) = seats.get(idx).copied() {
                        self.begin_cwd_pick(agent, seat);
                    }
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.seat_pick = Some((agent, step_pick(idx, n, true)));
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.seat_pick = Some((agent, step_pick(idx, n, false)));
                    return Ok(());
                }
                KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                    let i = (c as u8 - b'1') as usize;
                    if i < n {
                        self.seat_pick = None;
                        if let Some(seat) = seats.get(i).copied() {
                            self.begin_cwd_pick(agent, seat);
                            return Ok(());
                        }
                    }
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        if let Some(idx) = self.picking {
            let n = self.catalog.agents.len();
            match key.code {
                KeyCode::Esc => {
                    self.picking = None;
                    return Ok(());
                }
                KeyCode::Char('d') => {
                    if let Some(name) = self.catalog.agents.get(idx).map(|a| a.name.clone()) {
                        self.catalog.set_default(&name, &agents::default_root());
                    }
                    return Ok(());
                }
                KeyCode::Enter => {
                    self.picking = None;
                    if let Some(agent) = self.catalog.agents.get(idx).cloned() {
                        return self.confirm_agent(agent);
                    }
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.picking = Some(step_pick(idx, n, true));
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.picking = Some(step_pick(idx, n, false));
                    return Ok(());
                }
                KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                    let i = (c as u8 - b'1') as usize;
                    if i < n {
                        self.picking = None;
                        if let Some(agent) = self.catalog.agents.get(i).cloned() {
                            return self.confirm_agent(agent);
                        }
                    }
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        if self.notes.is_some() {
            let keep = self.notes.as_mut().is_some_and(|n| n.key(key));
            if !keep {
                if let Some(n) = self.notes.take() {
                    let window = n.window.clone();
                    let text = n.text();
                    match self.save_note(&window, &text) {
                        Ok(()) => {
                            self.last_error = None;
                            return self.refresh();
                        }
                        Err(err) => {
                            self.notes = Some(n);
                            self.last_error = Some(err.to_string());
                            return Ok(());
                        }
                    }
                }
            }
            return Ok(());
        }
        if self.prompting.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.prompting = None;
                    self.last_error = None;
                    return Ok(());
                }
                KeyCode::Enter => {
                    let text = self
                        .prompting
                        .as_ref()
                        .map(|b| b.trim().to_string())
                        .unwrap_or_default();
                    self.prompting = None;
                    if text.is_empty() {
                        return Ok(());
                    }
                    let pane = self.agent_pane();
                    return match self.call(Request::Write {
                        id: String::new(),
                        data: text,
                        pane,
                        prompt: true,
                    }) {
                        Ok(_) => {
                            self.last_error = None;
                            self.refresh()
                        }
                        Err(err) => {
                            self.last_error = Some(err.to_string());
                            Ok(())
                        }
                    };
                }
                KeyCode::Backspace => {
                    if let Some(buf) = self.prompting.as_mut() {
                        buf.pop();
                    }
                    self.last_error = None;
                    return Ok(());
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(buf) = self.prompting.as_mut() {
                        if buf.len() < 400 && !c.is_control() {
                            buf.push(c);
                        }
                    }
                    self.last_error = None;
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        if let Some(naming) = self.naming.as_ref() {
            let session = matches!(naming, Naming::Session(_));
            let buf = match naming {
                Naming::Window(buf) | Naming::Session(buf) => buf.clone(),
            };
            match key.code {
                KeyCode::Esc => {
                    self.naming = None;
                    self.last_error = None;
                    return Ok(());
                }
                KeyCode::Enter => {
                    let name = buf.trim().to_string();
                    if name.is_empty() {
                        self.naming = None;
                        self.last_error = None;
                        return Ok(());
                    }
                    let result = if session {
                        self.rename_session(&name)
                    } else {
                        self.rename_window(&name)
                    };
                    match result {
                        Ok(()) => {
                            self.naming = None;
                            self.last_error = None;
                            return self.refresh();
                        }
                        Err(err) => {
                            // Stay on the draft. Clearing naming here
                            // leaves last_error on the footer, and the
                            // next prefix-$ prompt eats keys while
                            // remaining invisible.
                            self.last_error = Some(err.to_string());
                            return Ok(());
                        }
                    }
                }
                KeyCode::Backspace => {
                    if let Some(naming) = self.naming.as_mut() {
                        match naming {
                            Naming::Window(b) | Naming::Session(b) => {
                                b.pop();
                            }
                        }
                    }
                    self.last_error = None;
                    return Ok(());
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(naming) = self.naming.as_mut() {
                        let buf = match naming {
                            Naming::Window(b) | Naming::Session(b) => b,
                        };
                        if buf.len() < 32 && !c.is_control() && c != '/' {
                            buf.push(c);
                        }
                    }
                    self.last_error = None;
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.which_key.set_scope(Scope::Prefix);
            self.which_key.toggle();
            return Ok(());
        }
        if self.which_key.active || self.which_key.is_pending() {
            if key.code == KeyCode::Esc {
                self.which_key.set_scope(Scope::Global);
                self.which_key.dismiss();
                return Ok(());
            }
            // `$` is prefix-$ (tmux rename-session). Terminals send
            // Char('$') or Shift-4; which-key's bind string skips `$`.
            if matches!(key.code, KeyCode::Char('$'))
                || (key.code == KeyCode::Char('4') && key.modifiers.contains(KeyModifiers::SHIFT))
            {
                return self.dispatch(Action::RenameSession);
            }
            // `&` is prefix-& (tmux kill-window). Terminals send
            // Char('&') or Shift-7.
            if matches!(key.code, KeyCode::Char('&'))
                || (key.code == KeyCode::Char('7') && key.modifiers.contains(KeyModifiers::SHIFT))
            {
                return self.dispatch(Action::CloseWindow);
            }
            if let Some(action) = self.which_key.handle_key(key) {
                return self.dispatch(action);
            }
            return Ok(());
        }
        if let Some(seq) = encode_passthrough(&key, self.key_proto()) {
            self.selection = None;
            self.forward(&seq);
        }
        Ok(())
    }

    fn key_proto(&self) -> (u16, bool) {
        let Some(view) = &self.view else {
            return (0, false);
        };
        self.grids
            .get(&view.focused)
            .map(|g| (g.kitty, g.modify))
            .unwrap_or((0, false))
    }

    /// Send a key to the focused pane's process. A pane whose process
    /// has ended is a normal state: the daemon closes it, the client
    /// reads the new view, and the key is dropped.
    fn forward(&mut self, data: &str) {
        if self.write(data).is_err() {
            let _ = self.refresh();
        }
    }

    /// An action is a wire op. Every branch sends the op, then reads
    /// the new state back. Help re-opens the prefix popup showing the
    /// bindings; Escape leaves it.
    fn dispatch(&mut self, action: Action) -> io::Result<()> {
        if matches!(action, Action::Help) {
            self.which_key.set_scope(Scope::Prefix);
            self.which_key.toggle();
            return Ok(());
        }
        let result = match action {
            Action::Detach => {
                self.detached = true;
                Ok(())
            }
            Action::ToggleSidebar => {
                self.sidebar.open = !self.sidebar.open;
                self.save_sidebar();
                let (w, h) = self.tty;
                self.resize_tty(w, h)?;
                self.refresh()
            }
            Action::NewSession => self.new_session(),
            Action::SwitchSession(n) => self.switch_session(n),
            Action::NewWindow => self.launch_terminal(),
            Action::NewAgent => {
                let agent = self.catalog.default_agent();
                self.launch_preferred(&agent)
            }
            Action::PickAgent => {
                self.picking = Some(0);
                Ok(())
            }
            Action::PickSession => {
                self.load_session_rows()?;
                let idx = self
                    .attached
                    .as_ref()
                    .and_then(|n| self.sessions.iter().position(|s| s == n))
                    .unwrap_or(0);
                self.sessions_pick = Some(idx);
                Ok(())
            }
            Action::RenameSession => {
                self.begin_naming(Naming::Session(self.session_label()));
                Ok(())
            }
            Action::RenameWindow => {
                let current = self.focused_window().unwrap_or_default();
                self.begin_naming(Naming::Window(current));
                if !self.sidebar.open {
                    self.sidebar.open = true;
                    self.save_sidebar();
                    let (w, h) = self.tty;
                    self.resize_tty(w, h)?;
                    self.refresh()?;
                }
                Ok(())
            }
            Action::NextWindow | Action::PrevWindow => {
                let next = matches!(action, Action::NextWindow);
                self.switch_window(next)?;
                self.refresh()
            }
            Action::SplitVertical | Action::SplitHorizontal => {
                let rows = matches!(action, Action::SplitHorizontal);
                let window = self.focused_window();
                if let Some(window) = window {
                    self.split(&window, rows)?;
                }
                self.refresh()
            }
            Action::FocusLeft => self.focus_dir(FocusDir::Left),
            Action::FocusRight => self.focus_dir(FocusDir::Right),
            Action::FocusUp => self.focus_dir(FocusDir::Up),
            Action::FocusDown => self.focus_dir(FocusDir::Down),
            Action::ClosePane => {
                if let Some(pane) = self.view.as_ref().map(|v| v.focused.clone()) {
                    self.call(Request::Close {
                        id: String::new(),
                        window: None,
                        pane: Some(pane),
                    })?;
                }
                self.refresh()
            }
            Action::CloseWindow => {
                if let Some(window) = self.focused_window() {
                    self.call(Request::Close {
                        id: String::new(),
                        window: Some(window),
                        pane: None,
                    })?;
                }
                self.refresh()
            }
            Action::Prompt => {
                if self.agent_pane().is_none() {
                    self.last_error = Some("no agent on this window".into());
                    return Ok(());
                }
                self.last_error = None;
                self.prompting = Some(String::new());
                Ok(())
            }
            Action::Notes => {
                let Some(window) = self.focused_window() else {
                    self.last_error = Some("no window".into());
                    return Ok(());
                };
                let text = self
                    .view
                    .as_ref()
                    .and_then(|v| v.windows.iter().find(|w| w.window == window))
                    .map(|w| w.note.clone())
                    .unwrap_or_default();
                self.last_error = None;
                self.notes = Some(Notes::open(window, &text));
                Ok(())
            }
            Action::Help => unreachable!(),
        };
        self.which_key.set_scope(Scope::Global);
        self.which_key.dismiss();
        result
    }

    /// Focus the neighboring pane in a direction, read from the view's
    /// geometry. The client is a viewer — it picks the adjacent pane
    /// and asks the daemon to focus it.
    fn focus_dir(&mut self, dir: FocusDir) -> io::Result<()> {
        let Some(target) = self.neighbor(dir) else {
            return Ok(());
        };
        self.call(Request::Focus {
            id: String::new(),
            window: None,
            pane: Some(target),
        })?;
        self.refresh()
    }

    /// The pane adjacent to the focused one in a direction, from the
    /// view's geometry. Direction means the closest tile across the
    /// shared edge that overlaps the focused tile.
    fn neighbor(&self, dir: FocusDir) -> Option<String> {
        let view = self.view.as_ref()?;
        let current = view
            .windows
            .iter()
            .find(|w| w.panes.iter().any(|p| p.pane == view.focused))?;
        let panes = &current.panes;
        let f = panes.iter().find(|p| p.pane == view.focused)?;
        let (fx, fy, fc, fr) = (f.x as i32, f.y as i32, f.cols as i32, f.rows as i32);

        let mut best: Option<(i32, &crate::daemon::session::PaneView)> = None;
        for p in panes {
            if p.pane == f.pane {
                continue;
            }
            let (px, py, pc, pr) = (p.x as i32, p.y as i32, p.cols as i32, p.rows as i32);
            // Rows (for left/right) or cols (for up/down) must overlap
            // the focused tile's span.
            let overlap = match dir {
                FocusDir::Left | FocusDir::Right => py < fy + fr && py + pr > fy,
                FocusDir::Up | FocusDir::Down => px < fx + fc && px + pc > fx,
            };
            if !overlap {
                continue;
            }
            // The neighbor's near edge and the focused tile's far edge
            // must face each other across the gap.
            let dist = match dir {
                FocusDir::Right if px >= fx + fc => px - (fx + fc),
                FocusDir::Left if px + pc <= fx => fx - (px + pc),
                FocusDir::Down if py >= fy + fr => py - (fy + fr),
                FocusDir::Up if py + pr <= fy => fy - (py + pr),
                _ => continue,
            };
            if best.as_ref().is_none_or(|(d, _)| dist < *d) {
                best = Some((dist, p));
            }
        }
        best.map(|(_, p)| p.pane.clone())
    }

    /// Focus the next or previous window, wrapping around.
    fn switch_window(&mut self, next: bool) -> io::Result<()> {
        let view = self
            .view
            .as_ref()
            .ok_or_else(|| io::Error::other("no session"))?;
        if view.windows.len() < 2 {
            return Ok(());
        }
        let current = self.focused_window().unwrap_or_default();
        let idx = view
            .windows
            .iter()
            .position(|w| w.window == current)
            .unwrap_or(0);
        let idx = if next {
            (idx + 1) % view.windows.len()
        } else {
            (idx + view.windows.len() - 1) % view.windows.len()
        };
        let window = view.windows[idx].window.clone();
        self.call(Request::Focus {
            id: String::new(),
            window: Some(window),
            pane: None,
        })?;
        Ok(())
    }

    /// A popup list eats the mouse: click a row to pick it, wheel
    /// moves like `j`/`k`, click outside cancels.
    fn picker_mouse(
        &mut self,
        col: u16,
        row: u16,
        kind: ratatui::crossterm::event::MouseEventKind,
    ) -> io::Result<bool> {
        use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEventKind};
        let area = Rect::new(0, 0, self.tty.0, self.tty.1);
        if let Some(pick) = &self.cwd_pick {
            let rows = self.cwd_rows(pick);
            let popup = pick_box(area, rows.len().max(4), 36, 64);
            match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if !pick_contains(popup, col, row) {
                        self.cwd_pick = None;
                        return Ok(true);
                    }
                    if let Some(i) = pick_row(pick_inner(popup), rows.len(), pick.sel, row) {
                        if let Some(p) = self.cwd_pick.as_mut() {
                            p.sel = i;
                        }
                    }
                }
                MouseEventKind::ScrollDown => {
                    if let Some(p) = self.cwd_pick.as_mut() {
                        if !rows.is_empty() {
                            p.sel = step_pick(p.sel, rows.len(), true);
                        }
                    }
                }
                MouseEventKind::ScrollUp => {
                    if let Some(p) = self.cwd_pick.as_mut() {
                        if !rows.is_empty() {
                            p.sel = step_pick(p.sel, rows.len(), false);
                        }
                    }
                }
                _ => {}
            }
            return Ok(true);
        }
        if let Some(notes) = self.notes.as_mut() {
            let popup = notes::notes_box(area);
            match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if pick_contains(popup, col, row) {
                        let inner = pick_inner(popup);
                        let visible = inner.height.max(1) as usize;
                        let scroll = notes.row().saturating_sub(visible.saturating_sub(1));
                        notes.click(inner, col, row, scroll);
                    }
                }
                MouseEventKind::ScrollDown => {
                    notes.key(ratatui::crossterm::event::KeyEvent::new(
                        KeyCode::Down,
                        ratatui::crossterm::event::KeyModifiers::NONE,
                    ));
                }
                MouseEventKind::ScrollUp => {
                    notes.key(ratatui::crossterm::event::KeyEvent::new(
                        KeyCode::Up,
                        ratatui::crossterm::event::KeyModifiers::NONE,
                    ));
                }
                _ => {}
            }
            return Ok(true);
        }
        if let Some(idx) = self.sessions_pick {
            let n = self.sessions.len();
            let popup = pick_box(area, n, 36, 64);
            match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if !pick_contains(popup, col, row) {
                        self.sessions_pick = None;
                        return Ok(true);
                    }
                    if let Some(i) = pick_row(pick_inner(popup), n, idx, row) {
                        self.sessions_pick = Some(i);
                    }
                }
                MouseEventKind::ScrollDown => {
                    self.sessions_pick = Some(step_pick(idx, n, true));
                }
                MouseEventKind::ScrollUp => {
                    self.sessions_pick = Some(step_pick(idx, n, false));
                }
                _ => {}
            }
            return Ok(true);
        }
        if let Some((agent, idx)) = self.seat_pick.clone() {
            let n = agent.seats().len();
            let popup = pick_box(area, n, 28, 48);
            match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if !pick_contains(popup, col, row) {
                        self.cancel_seat_pick();
                        return Ok(true);
                    }
                    if let Some(i) = pick_row(pick_inner(popup), n, idx, row) {
                        self.seat_pick = Some((agent, i));
                    }
                }
                MouseEventKind::ScrollDown => {
                    self.seat_pick = Some((agent, step_pick(idx, n, true)));
                }
                MouseEventKind::ScrollUp => {
                    self.seat_pick = Some((agent, step_pick(idx, n, false)));
                }
                _ => {}
            }
            return Ok(true);
        }
        if let Some(idx) = self.picking {
            let n = self.catalog.agents.len();
            let popup = pick_box(area, n, 28, 48);
            match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if !pick_contains(popup, col, row) {
                        self.picking = None;
                        return Ok(true);
                    }
                    if let Some(i) = pick_row(pick_inner(popup), n, idx, row) {
                        self.picking = Some(i);
                    }
                }
                MouseEventKind::ScrollDown => {
                    self.picking = Some(step_pick(idx, n, true));
                }
                MouseEventKind::ScrollUp => {
                    self.picking = Some(step_pick(idx, n, false));
                }
                _ => {}
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// Left click: a sidebar row focuses that window or agent; a tile
    /// focuses that pane.
    fn click(&mut self, col: u16, row: u16) -> io::Result<()> {
        let Some(view) = self.view.as_ref() else {
            return Ok(());
        };
        match hit(
            self.tty,
            self.sidebar.open,
            self.sidebar_cols(),
            self.sidebar.split,
            col,
            row,
            view,
            &self.recency,
        ) {
            Some(Hit::Window(window)) => {
                self.call(Request::Focus {
                    id: String::new(),
                    window: Some(window),
                    pane: None,
                })?;
                self.refresh()
            }
            Some(Hit::Pane(pane)) => {
                self.call(Request::Focus {
                    id: String::new(),
                    window: None,
                    pane: Some(pane),
                })?;
                self.refresh()
            }
            None => Ok(()),
        }
    }

    fn mouse(&mut self, ev: ratatui::crossterm::event::MouseEvent) -> io::Result<()> {
        use ratatui::crossterm::event::{MouseButton, MouseEventKind};
        let (col, row, kind) = (ev.column, ev.row, ev.kind);
        if self.picker_mouse(col, row, kind)? {
            return Ok(());
        }
        if self.drag.is_some() {
            return match kind {
                MouseEventKind::Drag(_) => self.mouse_drag(col, row),
                MouseEventKind::Up(_) => self.mouse_up(),
                _ => Ok(()),
            };
        }
        if self.selection.as_ref().is_some_and(|s| !s.finalized) {
            match kind {
                MouseEventKind::Drag(_) => return self.select_drag(col, row),
                MouseEventKind::Up(_) => return self.finish_select(),
                _ => {}
            }
        }
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.chrome_down(col, row)? {
                    return Ok(());
                }
                self.mouse_to_tile(col, row, kind, ev.modifiers)
            }
            MouseEventKind::Drag(_)
            | MouseEventKind::Up(_)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
            | MouseEventKind::Down(_) => self.mouse_to_tile(col, row, kind, ev.modifiers),
            _ => Ok(()),
        }
    }

    /// Sidebar chrome: drag handles and roster clicks. True if the
    /// event stays with the multiplexer.
    fn chrome_down(&mut self, col: u16, row: u16) -> io::Result<bool> {
        let kind = side(self.tty.0, self.sidebar.open);
        if kind == Side::Hidden {
            return Ok(false);
        }
        let sw = side_width(kind, self.sidebar_cols());
        let top = HEADER_LINES;
        let bot = self.tty.1.saturating_sub(STATUS_LINES);
        if row < top || row >= bot {
            return Ok(false);
        }
        let area = Rect::new(0, top, sw, bot.saturating_sub(top));
        if let Some(view) = self.view.as_ref() {
            let lay = side::layout(
                area,
                kind == Side::Open,
                self.sidebar.split,
                view,
                &self.recency,
            );
            if lay.divider_y == Some(row) && col < sw {
                self.drag = Some(Drag::Split);
                return Ok(true);
            }
            if col < sw && lay.at(row).is_some() {
                self.click(col, row)?;
                return Ok(true);
            }
        }
        if col + 1 == sw || col == sw {
            self.drag = Some(Drag::Width);
            return Ok(true);
        }
        Ok(false)
    }

    /// Focus the tile under the cursor. A pane that asked for mouse
    /// gets SGR. Otherwise (or with Shift) the client selects: drag
    /// copies to the clipboard.
    fn mouse_to_tile(
        &mut self,
        col: u16,
        row: u16,
        kind: ratatui::crossterm::event::MouseEventKind,
        modifiers: ratatui::crossterm::event::KeyModifiers,
    ) -> io::Result<()> {
        use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
        let Some(view) = self.view.as_ref() else {
            return Ok(());
        };
        let Some(tile) = tile_at(
            self.tty,
            self.sidebar.open,
            self.sidebar_cols(),
            col,
            row,
            view,
        ) else {
            return Ok(());
        };
        let pane = tile.pane.clone();
        let x = tile.x.saturating_sub(1);
        let y = tile.y.saturating_sub(1);
        if self.view.as_ref().is_some_and(|v| v.focused != pane) {
            self.call(Request::Focus {
                id: String::new(),
                window: None,
                pane: Some(pane.clone()),
            })?;
            self.refresh()?;
        }
        let mux = !mouse_for_pane(self.grids.get(&pane)) || modifiers.contains(KeyModifiers::SHIFT);
        if mux {
            return match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.selection = Some(select::Selection::begin(pane, x, y));
                    Ok(())
                }
                MouseEventKind::Drag(MouseButton::Left) => self.select_drag(col, row),
                MouseEventKind::Up(MouseButton::Left) => self.finish_select(),
                _ => Ok(()),
            };
        }
        self.selection = None;
        let Some(seq) = sgr_mouse(kind, tile.x, tile.y) else {
            return Ok(());
        };
        self.write(&seq)
    }

    fn select_drag(&mut self, col: u16, row: u16) -> io::Result<()> {
        let Some(view) = self.view.as_ref() else {
            return Ok(());
        };
        let Some(tile) = tile_at(
            self.tty,
            self.sidebar.open,
            self.sidebar_cols(),
            col,
            row,
            view,
        ) else {
            return Ok(());
        };
        if let Some(sel) = self.selection.as_mut() {
            if sel.pane == tile.pane {
                sel.drag(tile.x.saturating_sub(1), tile.y.saturating_sub(1));
            }
        }
        Ok(())
    }

    fn finish_select(&mut self) -> io::Result<()> {
        let Some(sel) = self.selection.as_mut() else {
            return Ok(());
        };
        if sel.was_just_click() {
            self.selection = None;
            return Ok(());
        }
        sel.finish();
        let sel = sel.clone();
        let Some(grid) = self.grids.get(&sel.pane) else {
            self.selection = None;
            return Ok(());
        };
        let Some(text) = select::extract(grid, &sel) else {
            self.selection = None;
            return Ok(());
        };
        if clip::write_text(&text) {
            self.toast = Some(Toast {
                message: "copied".into(),
                until: Instant::now() + Duration::from_millis(1500),
            });
        } else {
            self.last_error = Some("clipboard write failed".into());
            self.selection = None;
        }
        Ok(())
    }

    fn mouse_drag(&mut self, col: u16, row: u16) -> io::Result<()> {
        match self.drag {
            Some(Drag::Width) => {
                let cols = col.max(1);
                if cols <= RAIL_COLS {
                    if self.sidebar.open {
                        self.sidebar.open = false;
                    }
                } else {
                    self.sidebar.open = true;
                    self.sidebar.cols = cols.clamp(side::MIN_COLS, side::MAX_COLS);
                }
                let (w, h) = self.tty;
                self.resize_tty(w, h)
            }
            Some(Drag::Split) => {
                let top = HEADER_LINES;
                let h = self.tty.1.saturating_sub(CHROME_ROWS);
                if h > 2 {
                    let y = row.saturating_sub(top).min(h.saturating_sub(2));
                    self.sidebar.split = ((y as f32) / (h as f32)).clamp(0.2, 0.8);
                }
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn mouse_up(&mut self) -> io::Result<()> {
        if self.drag.take().is_some() {
            self.save_sidebar();
        }
        Ok(())
    }

    /// The focused pane if it is an agent, else the first named pane
    /// on the current window.
    fn agent_pane(&self) -> Option<String> {
        let view = self.view.as_ref()?;
        let focused = view
            .windows
            .iter()
            .flat_map(|w| w.panes.iter())
            .find(|p| p.pane == view.focused);
        if focused.and_then(|p| p.name.as_ref()).is_some() {
            return Some(view.focused.clone());
        }
        let win = view
            .windows
            .iter()
            .find(|w| w.panes.iter().any(|p| p.pane == view.focused))?;
        win.panes
            .iter()
            .find(|p| p.name.is_some())
            .map(|p| p.pane.clone())
    }

    fn focused_window(&self) -> Option<String> {
        let view = self.view.as_ref()?;
        for window in &view.windows {
            if window.panes.iter().any(|p| p.pane == view.focused) {
                return Some(window.window.clone());
            }
        }
        None
    }

    /// The theme's semantic tokens, resolved for this session.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn bump_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if self
            .toast
            .as_ref()
            .is_some_and(|t| t.until <= Instant::now())
        {
            self.toast = None;
        }
    }

    /// Draw one frame: fill the base, the sidebar, the panes' grids,
    /// the status line, and the prefix popup. The frame and the
    /// tiles share `bg.base`, so the gap between tiles is invisible —
    /// a single thin separator line marks the boundary.
    pub fn draw(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        frame.render_widget(Block::default().bg(self.c("bg.base")), area);

        let body = Rect {
            x: area.x,
            y: area.y + HEADER_LINES,
            width: area.width,
            height: area.height.saturating_sub(CHROME_ROWS),
        };
        match side(area.width, self.sidebar.open) {
            Side::Hidden => self.draw_content(frame, body),
            kind => {
                let chunks = Layout::horizontal([
                    Constraint::Length(side_width(kind, self.sidebar_cols())),
                    Constraint::Length(GAP),
                    Constraint::Fill(1),
                ])
                .split(body);
                self.draw_side(frame, chunks[0], kind);
                self.draw_content(frame, chunks[2]);
            }
        }
        self.draw_header(frame, area);
        self.draw_status(frame, area);
        if self.sessions_pick.is_some() {
            self.draw_sessions_popup(frame, area);
        }
        if self.seat_pick.is_some() {
            self.draw_seat_popup(frame, area);
        }
        if self.picking.is_some() {
            self.draw_agents_popup(frame, area);
        }
        if self.cwd_pick.is_some() {
            self.draw_cwd_popup(frame, area);
        }
        if self.notes.is_some() {
            self.draw_notes(frame, area);
        }

        if self.which_key.active || !self.which_key.current_sequence.is_empty() {
            let popup = WhichKey::new().border_style(Style::default().fg(self.c("border.focused")));
            popup.render(frame.buffer_mut(), &self.which_key);
        }
        self.draw_toast(frame, area);
    }

    fn draw_toast(&self, frame: &mut ratatui::Frame, area: Rect) {
        let Some(toast) = &self.toast else {
            return;
        };
        if toast.until <= Instant::now() {
            return;
        }
        let label = format!(" {} ", toast.message);
        let w = (label.chars().count() as u16 + 2).min(area.width.saturating_sub(2));
        let x = area.x + area.width.saturating_sub(w).saturating_sub(2);
        let y = area
            .y
            .saturating_add(area.height.saturating_sub(STATUS_LINES + 2));
        let box_area = Rect::new(x, y, w, 1);
        frame.render_widget(Clear, box_area);
        frame.render_widget(
            Paragraph::new(Span::styled(
                label,
                Style::default()
                    .fg(self.c("accent.primary"))
                    .bg(self.c("bg.elevated"))
                    .add_modifier(Modifier::BOLD),
            )),
            box_area,
        );
    }

    /// Windows above, agent processes below. The open sidebar writes
    /// two lines per entry: the name, then a clause.
    fn draw_side(&self, frame: &mut ratatui::Frame, area: Rect, kind: Side) {
        let open = kind == Side::Open;
        let bg = if open { "bg.panel" } else { "bg.base" };
        self.fill_rect(frame, area, self.c(bg));
        let Some(view) = &self.view else {
            return;
        };
        let lay = side::layout(area, open, self.sidebar.split, view, &self.recency);
        let panel = self.c(bg);
        let label = Style::default().fg(self.c("text.dim")).bg(panel);
        let border = Style::default().fg(self.c("border.subtle")).bg(panel);
        if let Some(y) = lay.windows_header {
            frame.buffer_mut().set_stringn(
                area.x + 1,
                y,
                "windows",
                area.width.saturating_sub(1) as usize,
                label,
            );
        }
        if let Some(y) = lay.agents_header {
            frame.buffer_mut().set_stringn(
                area.x + 1,
                y,
                "agents",
                area.width.saturating_sub(1) as usize,
                label,
            );
        }
        if let Some(y) = lay.divider_y {
            let grip = matches!(self.drag, Some(Drag::Split));
            let style = if grip {
                Style::default().fg(self.c("accent.primary"))
            } else {
                border
            };
            let line = "─".repeat(area.width.max(1) as usize);
            frame
                .buffer_mut()
                .set_stringn(area.x, y, &line, area.width as usize, style);
        }
        for (y, h, item) in &lay.hits {
            self.draw_side_item(frame, area, *y, *h, item, open, view, panel);
        }
    }

    fn draw_side_item(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
        y: u16,
        h: u16,
        item: &side::SideItem,
        open: bool,
        view: &SessionView,
        panel: ratatui::style::Color,
    ) {
        let focused_window = self.focused_window();
        let focused_pane = view.focused.as_str();
        let (here, mark, mark_style, title, clause) = match item {
            side::SideItem::Window(name) => {
                let w = view.windows.iter().find(|w| w.window == *name);
                let here = focused_window.as_deref() == Some(name.as_str());
                let (mark, mark_style) = match w {
                    Some(w) if side::window_has_agent(w) => self.state_mark(w.state),
                    _ => ("·", Style::default().fg(self.c("text.muted"))),
                };
                let clause = w.map(side::window_clause).unwrap_or_default();
                (here, mark, mark_style, name.clone(), clause)
            }
            side::SideItem::Agent {
                pane,
                window: _,
                name,
            } => {
                let here = focused_pane == pane;
                let state = view
                    .windows
                    .iter()
                    .flat_map(|w| w.panes.iter())
                    .find(|p| p.pane == *pane)
                    .map(|p| p.state)
                    .unwrap_or(WindowState::Idle);
                let (mark, mark_style) = self.state_mark(state);
                let pane_view = view
                    .windows
                    .iter()
                    .flat_map(|w| w.panes.iter())
                    .find(|p| p.pane == *pane);
                let activity = pane_view.and_then(|p| p.activity.as_deref());
                let session = pane_view.and_then(|p| p.session.as_deref());
                (
                    here,
                    mark,
                    mark_style,
                    name.clone(),
                    side::agent_clause(state, activity, session),
                )
            }
        };
        let accent = Style::default().fg(self.c("accent.primary")).bg(panel);
        let primary = Style::default().fg(self.c("text.primary")).bg(panel);
        let muted = Style::default().fg(self.c("text.muted")).bg(panel);
        let mark_style = mark_style.bg(panel);
        if here {
            frame.buffer_mut().set_stringn(area.x, y, "┃", 1, accent);
        }
        frame
            .buffer_mut()
            .set_stringn(area.x + 2, y, mark, 1, mark_style);
        if open {
            let title_style = if here { primary } else { muted };
            frame.buffer_mut().set_stringn(
                area.x + 4,
                y,
                &title,
                area.width.saturating_sub(4) as usize,
                title_style,
            );
            if h > 1 && y + 1 < area.bottom() {
                frame.buffer_mut().set_stringn(
                    area.x + 4,
                    y + 1,
                    &clause,
                    area.width.saturating_sub(4) as usize,
                    muted,
                );
            }
        }
    }

    fn state_mark(&self, state: WindowState) -> (&'static str, Style) {
        let muted = self.c("text.muted");
        match state {
            WindowState::NeedsYou => (MARK_NEED, Style::default().fg(self.c("error"))),
            WindowState::Turning => {
                let frame = DOT_FRAMES[(self.tick / 4) as usize % DOT_FRAMES.len()];
                (frame, Style::default().fg(muted))
            }
            WindowState::Dead => (MARK_DEAD, Style::default().fg(muted)),
            WindowState::Idle => (MARK_IDLE, Style::default().fg(muted)),
        }
    }

    /// The content: each pane's retained grid at its geometry. Panes
    /// with no process show a blank panel.
    fn draw_content(&self, frame: &mut ratatui::Frame, area: Rect) {
        let inner = area;
        let Some(view) = &self.view else {
            self.draw_home(frame, inner);
            return;
        };
        // A window is one screen: the current window — the one holding
        // the focused pane — is the only one drawn.
        let Some(current) = view
            .windows
            .iter()
            .find(|w| w.panes.iter().any(|p| p.pane == view.focused))
        else {
            return;
        };
        for pane in &current.panes {
            let rect = Rect {
                x: inner.x + pane.x,
                y: inner.y + pane.y,
                width: pane.cols.min(inner.width.saturating_sub(pane.x)),
                height: pane.rows.min(inner.height.saturating_sub(pane.y)),
            };
            self.draw_pane(frame, &pane.pane, rect, pane.pane == view.focused);
        }
        self.draw_separators(frame, inner, &current.panes);
    }

    /// The separators: a single thin line where a gap separates two
    /// tiles — `│` beside a column, `─` below a row, in the subtle
    /// border. The gap itself is invisible (the frame is `bg.base`).
    fn draw_separators(
        &self,
        frame: &mut ratatui::Frame,
        inner: Rect,
        panes: &[crate::daemon::session::PaneView],
    ) {
        let sep = Style::default().fg(self.c("border.subtle"));
        let bottom = frame.area().height;
        let right = frame.area().width;
        for pane in panes {
            // Beside a column: the gap column right of the tile.
            let gx = pane.x + pane.cols;
            if gx < inner.width && !cell_covered(panes, gx, pane.y) {
                let x = inner.x + gx;
                for dy in 0..pane.rows {
                    let y = inner.y + pane.y + dy;
                    if x < right && y < bottom {
                        frame.buffer_mut().set_stringn(x, y, "│", 1, sep);
                    }
                }
            }
            // Below a row: the gap row under the tile.
            let gy = pane.y + pane.rows;
            if gy < inner.height && !cell_covered(panes, pane.x, gy) {
                let y = inner.y + gy;
                for dx in 0..pane.cols {
                    let x = inner.x + pane.x + dx;
                    if x < right && y < bottom {
                        frame.buffer_mut().set_stringn(x, y, "─", 1, sep);
                    }
                }
            }
        }
    }

    /// The empty state: a centered wordmark, in the accent.
    fn draw_home(&self, frame: &mut ratatui::Frame, area: Rect) {
        let wordmark = "anvil";
        let subtitle = "no sessions — create one (ctrl-b n)";
        let y = area.y + area.height / 2;
        frame.buffer_mut().set_stringn(
            area.x
                .saturating_add(area.width / 2)
                .saturating_sub(wordmark.len() as u16 / 2),
            y,
            wordmark,
            area.width as usize,
            Style::default().fg(self.c("accent.primary")),
        );
        frame.buffer_mut().set_stringn(
            area.x
                .saturating_add(area.width / 2)
                .saturating_sub(subtitle.len() as u16 / 2),
            y + 1,
            subtitle,
            area.width as usize,
            Style::default().fg(self.c("text.muted")),
        );
    }

    /// A pane: its `bg.base` ground, then its grid styled by the runs
    /// the daemon kept. The focused pane keeps full brightness and its
    /// cursor; the other panes wear a dark veil over their cells.
    fn draw_pane(&self, frame: &mut ratatui::Frame, pane_id: &str, rect: Rect, focused: bool) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let Some(grid) = self.grids.get(pane_id) else {
            return;
        };
        frame.render_widget(Block::default().bg(self.c("bg.base")), rect);
        let mut lines: Vec<Line> = Vec::new();
        for (i, runs) in grid.runs.iter().enumerate() {
            if i >= rect.height as usize {
                break;
            }
            let spans: Vec<Span> = runs
                .iter()
                .map(|run| Span::styled(run.text.clone(), self.style(run)))
                .collect();
            lines.push(Line::from(spans));
        }
        while lines.len() < rect.height as usize {
            lines.push(Line::from(""));
        }
        if let Some(sel) = self.selection.as_ref().filter(|s| s.pane == pane_id) {
            let hi = Style::default()
                .fg(self.c("text.primary"))
                .bg(self.c("bg.selection"));
            lines = select::highlight(&lines, sel, hi);
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), rect);
        if !focused {
            self.darken_tile(frame, rect);
        }
        if focused && grid.alive {
            let col = grid.cursor_col;
            let row = grid.cursor_row;
            if row < rect.height && col < rect.width {
                frame.set_cursor_position((rect.x + col, rect.y + row));
            }
        }
    }

    /// Darken every cell in a tile: the inactive tile's veil. It is a
    /// plain brightness shift on the colors already in the buffer, so
    /// it needs no knowledge of the theme.
    fn darken_tile(&self, frame: &mut ratatui::Frame, rect: Rect) {
        const FG: f32 = 0.75;
        const BG: f32 = 0.65;
        for x in rect.left()..rect.right() {
            for y in rect.top()..rect.bottom() {
                if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                    cell.fg = darken(cell.fg, FG);
                    cell.bg = darken(cell.bg, BG);
                }
            }
        }
    }

    fn session_index(&self) -> usize {
        self.attached
            .as_ref()
            .and_then(|n| self.sessions.iter().position(|s| s == n))
            .unwrap_or(0)
            + 1
    }

    /// The chip is the session's name, or its index when the name is
    /// a leftover default (`main`).
    fn session_label(&self) -> String {
        let name = self.attached.as_deref().unwrap_or("");
        display_session(name, self.session_index())
    }

    /// Session chip on the left, host on the right. No window name.
    fn draw_header(&self, frame: &mut ratatui::Frame, area: Rect) {
        let bar = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: HEADER_LINES,
        };
        let panel = self.c("bg.panel");
        self.fill_rect(frame, bar, panel);
        let chip = format!(" {} ", self.session_label());
        frame.buffer_mut().set_stringn(
            area.x + PAD,
            bar.y,
            &chip,
            chip.chars().count(),
            Style::default()
                .fg(self.c("bg.base"))
                .bg(self.c("accent.primary")),
        );
        let host = &self.host;
        let hx = area
            .right()
            .saturating_sub(PAD + host.chars().count() as u16);
        if hx + host.chars().count() as u16 <= area.right() {
            frame.buffer_mut().set_stringn(
                hx,
                bar.y,
                host,
                host.chars().count(),
                Style::default().fg(self.c("text.dim")).bg(panel),
            );
        }
        let chip_end = area.x + PAD + chip.chars().count() as u16 + 2;
        let host_start = hx.saturating_sub(2);
        if let Some(track) = sat::header_rect(chip_end, host_start, bar.y) {
            if let Some(counters) = self.sat_header() {
                self.paint_sat_header(frame, track, &counters);
            }
        }
    }

    /// Stamp chrome so pane glyphs cannot show through. Block only
    /// restyles; it leaves whatever symbol the tiles wrote.
    fn fill_rect(&self, frame: &mut ratatui::Frame, bar: Rect, bg: ratatui::style::Color) {
        let buf = frame.buffer_mut();
        for y in bar.y..bar.bottom() {
            for x in bar.x..bar.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_symbol(" ");
                    cell.set_fg(self.c("text.dim"));
                    cell.set_bg(bg);
                }
            }
        }
    }

    /// Instant from the live view when the file is empty or stale;
    /// 24h stain still comes from the daemon file.
    fn sat_header(&self) -> Option<crate::daemon::sat::Counters> {
        let mut c = self.sat.all.clone();
        let live = self.view.as_ref().map(crate::daemon::sat::count_view);
        if let Some((busy, agents)) = live {
            if let Some(prev) = self.attached.as_ref().and_then(|n| self.sat.session(n)) {
                c.busy = c.busy.saturating_sub(prev.busy) + busy;
                c.agents = c.agents.saturating_sub(prev.agents) + agents;
            } else if c.agents == 0 {
                c.busy = busy;
                c.agents = agents;
            }
        }
        (c.agents > 0).then_some(c)
    }

    fn paint_sat_header(
        &self,
        frame: &mut ratatui::Frame,
        track: Rect,
        counters: &crate::daemon::sat::Counters,
    ) {
        sat::draw_header(
            frame.buffer_mut(),
            track,
            counters,
            self.c("bg.panel"),
            self.c(sat::hue_token()),
            self.c("bg.panel"),
        );
    }

    fn focused_needs_you(&self) -> bool {
        let Some(view) = &self.view else {
            return false;
        };
        view.windows
            .iter()
            .flat_map(|w| w.panes.iter())
            .find(|p| p.pane == view.focused)
            .is_some_and(|p| p.state == WindowState::NeedsYou)
    }

    fn window_count(&self) -> usize {
        self.view.as_ref().map(|v| v.windows.len()).unwrap_or(0)
    }

    fn pane_count(&self) -> usize {
        let Some(view) = &self.view else {
            return 0;
        };
        view.windows
            .iter()
            .find(|w| w.panes.iter().any(|p| p.pane == view.focused))
            .map(|w| w.panes.len())
            .unwrap_or(0)
    }

    /// Keys that apply right now — not a catalog.
    fn footer_hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.which_key.active || self.which_key.is_pending() {
            return vec![("esc", "cancel")];
        }
        if self.sessions_pick.is_some() {
            return vec![
                ("j/k", "move"),
                ("click", "pick"),
                ("enter", "switch"),
                ("n", "new"),
                ("x", "drop"),
                ("esc", "cancel"),
            ];
        }
        if self.seat_pick.is_some() {
            return vec![
                ("j/k", "move"),
                ("click", "pick"),
                ("enter", "launch"),
                ("esc", "back"),
            ];
        }
        if self.picking.is_some() {
            return vec![
                ("j/k", "move"),
                ("click", "pick"),
                ("enter", "launch"),
                ("d", "default"),
                ("esc", "cancel"),
            ];
        }
        if let Some(notes) = &self.notes {
            let mut hints = vec![("enter", "newline"), ("esc", "save"), ("↑↓", "move")];
            if notes.on_task() {
                hints.insert(0, ("space", "check"));
            }
            return hints;
        }
        if self.cwd_pick.is_some() {
            return vec![("enter", "launch"), ("tab", "complete"), ("esc", "cancel")];
        }
        if self.naming.is_some() || self.prompting.is_some() {
            return vec![("enter", "ok"), ("esc", "cancel")];
        }
        if matches!(self.drag, Some(Drag::Width)) {
            return vec![("drag", "width")];
        }
        if matches!(self.drag, Some(Drag::Split)) {
            return vec![("drag", "split")];
        }
        if self.focused_needs_you() {
            return vec![("y", "allow"), ("n", "deny")];
        }
        let mut hints = Vec::new();
        if self.sidebar.open {
            hints.push(("ctrl-b w", "hide windows"));
        } else {
            hints.push(("ctrl-b w", "show windows"));
        }
        hints.push(("ctrl-b a", "new agent"));
        hints.push(("ctrl-b c", "new shell"));
        hints.push(("ctrl-b m", "notes"));
        if self.window_count() > 1 {
            hints.push(("ctrl-b ]", "next"));
        }
        if self.pane_count() > 1 {
            hints.push(("ctrl-b h", "pane"));
        }
        if self.sessions.len() > 1 {
            hints.push(("ctrl-b s", "sessions"));
        }
        hints.push(("ctrl-b q", "detach"));
        hints
    }

    /// Footer: contextual key (bold) + `:desc` (muted), centered.
    fn draw_status(&self, frame: &mut ratatui::Frame, area: Rect) {
        let y = area.bottom().saturating_sub(1);
        let bar = Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        };
        let asking = self.naming.is_some() || self.prompting.is_some() || self.cwd_pick.is_some();
        let bar_bg = if asking { "error" } else { "bg.panel" };
        let panel = self.c(bar_bg);
        self.fill_rect(frame, bar, panel);
        if error_owns_footer(self.naming.as_ref(), self.last_error.as_deref())
            && self.prompting.is_none()
        {
            if let Some(err) = &self.last_error {
                frame.buffer_mut().set_stringn(
                    area.x + PAD,
                    y,
                    err,
                    (area.width.saturating_sub(2 * PAD)) as usize,
                    Style::default().fg(self.c("error")).bg(panel),
                );
                return;
            }
        }
        let key = if asking {
            Style::default()
                .fg(self.c("bg.base"))
                .bg(self.c("error"))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.c("text.muted"))
                .bg(panel)
                .add_modifier(Modifier::BOLD)
        };
        let desc = if asking {
            Style::default().fg(self.c("bg.base")).bg(self.c("error"))
        } else {
            Style::default().fg(self.c("text.dim")).bg(panel)
        };
        let pipe = if asking {
            Style::default().fg(self.c("bg.base")).bg(self.c("error"))
        } else {
            Style::default().fg(self.c("text.dim")).bg(panel)
        };
        let mut spans: Vec<Span> = Vec::new();
        if self.sessions_pick.is_some()
            || self.picking.is_some()
            || self.seat_pick.is_some()
            || self.notes.is_some()
        {
            // The popup is the list; the footer is only keys.
        } else if let Some(draft) = &self.prompting {
            spans.push(Span::styled(format!("prompt: {draft}_"), key));
            if let Some(err) = &self.last_error {
                spans.push(Span::styled("  |  ", pipe));
                spans.push(Span::styled(err.clone(), key));
            }
            spans.push(Span::styled("  |  ", pipe));
        } else if let Some(naming) = &self.naming {
            let draft = match naming {
                Naming::Window(d) | Naming::Session(d) => d,
            };
            let kind = match naming {
                Naming::Session(_) => "session",
                Naming::Window(_) => "window",
            };
            spans.push(Span::styled(format!("rename {kind}: {draft}_"), key));
            if let Some(err) = &self.last_error {
                spans.push(Span::styled("  |  ", pipe));
                spans.push(Span::styled(err.clone(), key));
            }
            spans.push(Span::styled("  |  ", pipe));
        }
        for (i, (bind, label)) in self.footer_hints().iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" | ", pipe));
            }
            spans.push(Span::styled(*bind, key));
            spans.push(Span::styled(format!(":{label}"), desc));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
            bar,
        );
    }

    fn draw_agents_popup(&self, frame: &mut ratatui::Frame, area: Rect) {
        let Some(sel) = self.picking else {
            return;
        };
        let agents = &self.catalog.agents;
        let popup = pick_box(area, agents.len(), 28, 48);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Block::default()
                .title(" agents ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.c("border.focused")))
                .bg(self.c("bg.panel")),
            popup,
        );
        let inner = pick_inner(popup);
        let visible = (inner.height / PICK_ROW).max(1) as usize;
        let scroll = sel.saturating_sub(visible.saturating_sub(1));
        for (n, agent) in agents.iter().enumerate().skip(scroll).take(visible) {
            let y = inner.y + ((n - scroll) as u16) * PICK_ROW;
            if y >= inner.bottom() {
                break;
            }
            let here = agent.name == self.catalog.default;
            let picked = n == sel;
            let title_style = if picked {
                Style::default()
                    .fg(self.c("text.primary"))
                    .add_modifier(Modifier::BOLD)
            } else if here {
                Style::default().fg(self.c("accent.primary"))
            } else {
                Style::default().fg(self.c("text.muted"))
            };
            if picked {
                frame.render_widget(
                    Block::default().bg(self.c("bg.elevated")),
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: PICK_ROW.min(inner.bottom().saturating_sub(y)),
                    },
                );
            }
            let mark = if here { "┃" } else { " " };
            frame.buffer_mut().set_stringn(
                inner.x,
                y,
                mark,
                1,
                Style::default().fg(self.c("accent.primary")),
            );
            frame.buffer_mut().set_stringn(
                inner.x + 2,
                y,
                &agent.name,
                inner.width.saturating_sub(2) as usize,
                title_style,
            );
            if y + 1 < inner.bottom() {
                let clause = if here {
                    "default".to_string()
                } else {
                    agent_clause(agent)
                };
                frame.buffer_mut().set_stringn(
                    inner.x + 2,
                    y + 1,
                    &clause,
                    inner.width.saturating_sub(2) as usize,
                    Style::default().fg(self.c("text.dim")),
                );
            }
        }
    }

    fn draw_seat_popup(&self, frame: &mut ratatui::Frame, area: Rect) {
        let Some((agent, sel)) = &self.seat_pick else {
            return;
        };
        let seats = agent.seats();
        let popup = pick_box(area, seats.len(), 28, 48);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Block::default()
                .title(format!(" {} ", agent.name))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.c("border.focused")))
                .bg(self.c("bg.panel")),
            popup,
        );
        let inner = pick_inner(popup);
        let visible = (inner.height / PICK_ROW).max(1) as usize;
        let scroll = sel.saturating_sub(visible.saturating_sub(1));
        for (n, seat) in seats.iter().enumerate().skip(scroll).take(visible) {
            let y = inner.y + ((n - scroll) as u16) * PICK_ROW;
            if y >= inner.bottom() {
                break;
            }
            let picked = n == *sel;
            let title_style = if picked {
                Style::default()
                    .fg(self.c("text.primary"))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.c("text.muted"))
            };
            if picked {
                frame.render_widget(
                    Block::default().bg(self.c("bg.elevated")),
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: PICK_ROW.min(inner.bottom().saturating_sub(y)),
                    },
                );
            }
            frame.buffer_mut().set_stringn(
                inner.x + 2,
                y,
                seat.label(),
                inner.width.saturating_sub(2) as usize,
                title_style,
            );
            if y + 1 < inner.bottom() {
                let clause = match seat {
                    Seat::Native => agent.program.as_str(),
                    Seat::Anvil => agent.acp_cmd().unwrap_or("acp"),
                };
                frame.buffer_mut().set_stringn(
                    inner.x + 2,
                    y + 1,
                    clause,
                    inner.width.saturating_sub(2) as usize,
                    Style::default().fg(self.c("text.dim")),
                );
            }
        }
    }

    fn draw_cwd_popup(&self, frame: &mut ratatui::Frame, area: Rect) {
        let Some(pick) = &self.cwd_pick else {
            return;
        };
        let rows = self.cwd_rows(pick);
        let popup = pick_box(area, rows.len().max(4) + 1, 36, 64);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Block::default()
                .title(format!(" {} directory ", pick.agent.name))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.c("border.focused")))
                .bg(self.c("bg.panel")),
            popup,
        );
        let inner = pick_inner(popup);
        let draft = cwd::display(&pick.draft);
        frame.buffer_mut().set_stringn(
            inner.x,
            inner.y,
            &draft,
            inner.width as usize,
            Style::default()
                .fg(self.c("text.primary"))
                .add_modifier(Modifier::UNDERLINED),
        );
        let list_y = inner.y.saturating_add(2);
        let list_h = inner.bottom().saturating_sub(list_y);
        let visible = (list_h / PICK_ROW).max(1) as usize;
        let scroll = pick.sel.saturating_sub(visible.saturating_sub(1));
        for (n, row) in rows.iter().enumerate().skip(scroll).take(visible) {
            let y = list_y + ((n - scroll) as u16) * PICK_ROW;
            if y >= inner.bottom() {
                break;
            }
            let picked = n == pick.sel;
            if picked {
                frame.render_widget(
                    Block::default().bg(self.c("bg.elevated")),
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: PICK_ROW.min(inner.bottom().saturating_sub(y)),
                    },
                );
            }
            let title_style = if picked {
                Style::default()
                    .fg(self.c("text.primary"))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.c("text.muted"))
            };
            frame.buffer_mut().set_stringn(
                inner.x + 2,
                y,
                &cwd::display(&row.path),
                inner.width.saturating_sub(2) as usize,
                title_style,
            );
            if y + 1 < inner.bottom() {
                frame.buffer_mut().set_stringn(
                    inner.x + 2,
                    y + 1,
                    row.kind.clause(),
                    inner.width.saturating_sub(2) as usize,
                    Style::default().fg(self.c("text.dim")),
                );
            }
        }
    }

    fn draw_notes(&self, frame: &mut ratatui::Frame, area: Rect) {
        let Some(notes) = &self.notes else {
            return;
        };
        let popup = notes::notes_box(area);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Block::default()
                .title(format!(" {} ", notes.window))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.c("border.focused")))
                .bg(self.c("bg.panel")),
            popup,
        );
        let inner = pick_inner(popup);
        let visible = inner.height.max(1) as usize;
        let scroll = notes.row().saturating_sub(visible.saturating_sub(1));
        for i in scroll..notes.line_count().min(scroll + visible) {
            let y = inner.y + (i - scroll) as u16;
            if y >= inner.bottom() {
                break;
            }
            let line = notes.line(i);
            let here = i == notes.row();
            let bg = if here {
                self.c("bg.elevated")
            } else {
                self.c("bg.panel")
            };
            if here {
                frame.render_widget(
                    Block::default().bg(bg),
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: 1,
                    },
                );
            }
            let body = Style::default().fg(self.c("text.primary")).bg(bg);
            frame
                .buffer_mut()
                .set_stringn(inner.x, y, line, inner.width as usize, body);
            if let Some((at, checked)) = notes::task_box(line) {
                let mark = if checked { "[x]" } else { "[ ]" };
                let box_style = Style::default()
                    .fg(if checked {
                        self.c("accent.primary")
                    } else {
                        self.c("text.muted")
                    })
                    .bg(bg)
                    .add_modifier(Modifier::BOLD);
                frame.buffer_mut().set_stringn(
                    inner.x.saturating_add(at as u16),
                    y,
                    mark,
                    inner.width.saturating_sub(at as u16) as usize,
                    box_style,
                );
            }
            if here {
                let cursor_x = inner.x.saturating_add(notes.col() as u16);
                if cursor_x < inner.right() {
                    frame.buffer_mut()[(cursor_x, y)]
                        .set_style(Style::default().add_modifier(Modifier::REVERSED));
                }
            }
        }
    }

    fn draw_sessions_popup(&self, frame: &mut ratatui::Frame, area: Rect) {
        let Some(sel) = self.sessions_pick else {
            return;
        };
        let rows = &self.session_rows;
        let popup = pick_box(area, rows.len(), 36, 64);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Block::default()
                .title(" sessions ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.c("border.focused")))
                .bg(self.c("bg.panel")),
            popup,
        );
        let inner = pick_inner(popup);
        let visible = (inner.height / PICK_ROW).max(1) as usize;
        let scroll = sel.saturating_sub(visible.saturating_sub(1));
        let current = self.attached.as_deref();
        for (n, row) in rows.iter().enumerate().skip(scroll).take(visible) {
            let y = inner.y + ((n - scroll) as u16) * PICK_ROW;
            if y >= inner.bottom() {
                break;
            }
            let here = current == Some(row.name.as_str());
            let picked = n == sel;
            let title_style = if picked {
                Style::default()
                    .fg(self.c("text.primary"))
                    .add_modifier(Modifier::BOLD)
            } else if here {
                Style::default().fg(self.c("accent.primary"))
            } else {
                Style::default().fg(self.c("text.muted"))
            };
            if picked {
                frame.render_widget(
                    Block::default().bg(self.c("bg.elevated")),
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: PICK_ROW.min(inner.bottom().saturating_sub(y)),
                    },
                );
            }
            let mark = if here { "┃" } else { " " };
            frame.buffer_mut().set_stringn(
                inner.x,
                y,
                mark,
                1,
                Style::default().fg(self.c("accent.primary")),
            );
            frame.buffer_mut().set_stringn(
                inner.x + 2,
                y,
                &row.title,
                inner.width.saturating_sub(2) as usize,
                title_style,
            );
            if y + 1 < inner.bottom() {
                frame.buffer_mut().set_stringn(
                    inner.x + 2,
                    y + 1,
                    &row.clause,
                    inner.width.saturating_sub(2) as usize,
                    Style::default().fg(self.c("text.dim")),
                );
            }
        }
    }

    fn c(&self, token: &str) -> ratatui::style::Color {
        self.theme.color(token).into()
    }

    fn style(&self, run: &crate::daemon::pane::Run) -> Style {
        let mut style = Style::default();
        if let Some(fg) = run.fg {
            style = style.fg(Color::Indexed(fg));
        } else if let Some([r, g, b]) = run.fg_rgb {
            style = style.fg(Color::Rgb(r, g, b));
        } else {
            // The client is the pane's terminal: default text is the
            // theme's, so an inactive tile can darken it.
            style = style.fg(self.c("text.primary"));
        }
        if let Some(bg) = run.bg {
            style = style.bg(Color::Indexed(bg));
        } else if let Some([r, g, b]) = run.bg_rgb {
            style = style.bg(Color::Rgb(r, g, b));
        }
        if run.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if run.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if run.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        if run.inverse {
            style = style.add_modifier(Modifier::REVERSED);
        }
        style
    }
}

/// A left-click target.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    Window(String),
    Pane(String),
}

/// Map a terminal cell to a window row or a pane tile.
fn hit(
    tty: (u16, u16),
    open: bool,
    cols: u16,
    split: f32,
    col: u16,
    row: u16,
    view: &SessionView,
    recency: &HashMap<String, side::Recency>,
) -> Option<Hit> {
    let (tw, th) = tty;
    if row < HEADER_LINES || row >= th.saturating_sub(STATUS_LINES) {
        return None;
    }
    let kind = side(tw, open);
    let sw = side_width(kind, cols);
    if kind != Side::Hidden && col < sw {
        let area = Rect::new(0, HEADER_LINES, sw, th.saturating_sub(CHROME_ROWS));
        return match side::layout(area, kind == Side::Open, split, view, recency).hit(row) {
            Some(side::SideHit::Window(w)) => Some(Hit::Window(w)),
            Some(side::SideHit::Pane(p)) => Some(Hit::Pane(p)),
            None => None,
        };
    }
    tile_at(tty, open, cols, col, row, view).map(|t| Hit::Pane(t.pane))
}

/// A cell inside a tile, in the pane's own 1-based coordinates.
struct TileAt {
    pane: String,
    x: u16,
    y: u16,
}

fn tile_at(
    tty: (u16, u16),
    open: bool,
    cols: u16,
    col: u16,
    row: u16,
    view: &SessionView,
) -> Option<TileAt> {
    let (tw, th) = tty;
    if row < HEADER_LINES || row >= th.saturating_sub(STATUS_LINES) {
        return None;
    }
    let kind = side(tw, open);
    let sw = side_width(kind, cols);
    if kind != Side::Hidden && col < sw {
        return None;
    }
    let inner_x = if kind == Side::Hidden { 0 } else { sw + GAP };
    let inner_y = HEADER_LINES;
    let current = view
        .windows
        .iter()
        .find(|w| w.panes.iter().any(|p| p.pane == view.focused))?;
    for pane in &current.panes {
        let px = inner_x + pane.x;
        let py = inner_y + pane.y;
        if col >= px && col < px + pane.cols && row >= py && row < py + pane.rows {
            return Some(TileAt {
                pane: pane.pane.clone(),
                x: col - px + 1,
                y: row - py + 1,
            });
        }
    }
    None
}

/// Keys that belong to the pane. Ctrl-C is `^C` (`\x03`), not the
/// letter `c`. Ctrl-b is the prefix and is consumed before this.
/// `kitty` / `modify` are what the focused process asked for, so
/// Shift+Enter can be a newline in OpenCode instead of submit.
fn encode_passthrough(
    key: &ratatui::crossterm::event::KeyEvent,
    proto: (u16, bool),
) -> Option<String> {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    let (kitty, modify) = proto;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let m = 1u8 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let seq = match key.code {
        KeyCode::Esc => "\x1b".into(),
        KeyCode::Enter => encode_enter(m, kitty, modify),
        KeyCode::Backspace => "\u{7f}".into(),
        KeyCode::Tab => encode_tab(m, kitty, modify),
        KeyCode::Delete => encode_csi_tilde(3, m),
        KeyCode::Home => encode_csi_letter('H', m),
        KeyCode::End => encode_csi_letter('F', m),
        KeyCode::PageUp => encode_csi_tilde(5, m),
        KeyCode::PageDown => encode_csi_tilde(6, m),
        KeyCode::Up => encode_csi_letter('A', m),
        KeyCode::Down => encode_csi_letter('B', m),
        KeyCode::Right => encode_csi_letter('C', m),
        KeyCode::Left => encode_csi_letter('D', m),
        KeyCode::Char(c) if ctrl => ctrl_char(c)?.to_string(),
        KeyCode::Char(c) if alt => format!("\x1b{c}"),
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).to_string()
        }
        _ => return None,
    };
    Some(seq)
}

fn encode_enter(m: u8, kitty: u16, modify: bool) -> String {
    if m == 1 {
        return "\r".into();
    }
    if kitty > 0 {
        return format!("\x1b[13;{m}u");
    }
    if modify {
        return format!("\x1b[27;{m};13~");
    }
    "\r".into()
}

fn encode_tab(m: u8, kitty: u16, modify: bool) -> String {
    if m == 1 {
        return "\t".into();
    }
    if kitty > 0 {
        return format!("\x1b[9;{m}u");
    }
    if modify {
        return format!("\x1b[27;{m};9~");
    }
    if m == 2 {
        return "\x1b[Z".into();
    }
    "\t".into()
}

fn encode_csi_letter(letter: char, m: u8) -> String {
    if m == 1 {
        format!("\x1b[{letter}")
    } else {
        format!("\x1b[1;{m}{letter}")
    }
}

fn encode_csi_tilde(n: u8, m: u8) -> String {
    if m == 1 {
        format!("\x1b[{n}~")
    } else {
        format!("\x1b[{n};{m}~")
    }
}

fn ctrl_char(c: char) -> Option<char> {
    if c.is_ascii_control() && c != '\0' {
        return Some(c);
    }
    let c = c.to_ascii_lowercase();
    Some(match c {
        'a'..='z' => (c as u8 - b'a' + 1) as char,
        ' ' | '@' => '\x00',
        '[' => '\x1b',
        '\\' => '\x1c',
        ']' => '\x1d',
        '^' | '6' => '\x1e',
        '_' | '-' | '/' => '\x1f',
        '?' => '\x7f',
        _ => return None,
    })
}

/// The process asked for mouse (DECSET 1000/1002/1003). A shell has not.
fn mouse_for_pane(grid: Option<&Grid>) -> bool {
    grid.is_some_and(|g| g.mouse)
}

/// xterm SGR mouse (`CSI < btn ; x ; y M/m`), 1-based, pane-local.
fn sgr_mouse(kind: ratatui::crossterm::event::MouseEventKind, x: u16, y: u16) -> Option<String> {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let (btn, release) = match kind {
        MouseEventKind::Down(MouseButton::Left) => (0, false),
        MouseEventKind::Down(MouseButton::Middle) => (1, false),
        MouseEventKind::Down(MouseButton::Right) => (2, false),
        MouseEventKind::Up(MouseButton::Left) => (0, true),
        MouseEventKind::Up(MouseButton::Middle) => (1, true),
        MouseEventKind::Up(MouseButton::Right) => (2, true),
        MouseEventKind::Drag(MouseButton::Left) => (32, false),
        MouseEventKind::Drag(MouseButton::Middle) => (33, false),
        MouseEventKind::Drag(MouseButton::Right) => (34, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        MouseEventKind::ScrollLeft => (66, false),
        MouseEventKind::ScrollRight => (67, false),
        _ => return None,
    };
    let end = if release { 'm' } else { 'M' };
    Some(format!("\x1b[<{btn};{x};{y}{end}"))
}

pub(crate) fn is_legacy_session(name: &str) -> bool {
    name == "main"
        || name
            .strip_prefix("main-")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn unused_session_name(taken: &[String]) -> String {
    let mut n = 1u32;
    loop {
        let candidate = n.to_string();
        if !taken.iter().any(|t| t == &candidate) {
            return candidate;
        }
        n += 1;
    }
}

pub(crate) fn display_session(name: &str, index: usize) -> String {
    if name.is_empty() || is_legacy_session(name) {
        index.to_string()
    } else {
        name.to_string()
    }
}

/// Whether a cell is inside any pane's rectangle, in canvas coords.
fn cell_covered(panes: &[crate::daemon::session::PaneView], x: u16, y: u16) -> bool {
    panes
        .iter()
        .any(|p| x >= p.x && x < p.x + p.cols && y >= p.y && y < p.y + p.rows)
}

/// Scale a color toward black. Indexed palette colors resolve to the
/// 256-color cube first, so the shift is the same for every cell.
fn darken(color: Color, factor: f32) -> Color {
    let [r, g, b] = match color {
        Color::Rgb(r, g, b) => [r, g, b],
        Color::Indexed(n) => indexed_rgb(n),
        _ => return color,
    };
    let scale = |v: u8| ((v as f32) * factor).round() as u8;
    Color::Rgb(scale(r), scale(g), scale(b))
}

/// The standard 256-color cube, as RGB.
fn indexed_rgb(n: u8) -> [u8; 3] {
    match n {
        0..=15 => {
            let base: [[u8; 3]; 16] = [
                [0, 0, 0],
                [128, 0, 0],
                [0, 128, 0],
                [128, 128, 0],
                [0, 0, 128],
                [128, 0, 128],
                [0, 128, 128],
                [192, 192, 192],
                [128, 128, 128],
                [255, 0, 0],
                [0, 255, 0],
                [255, 255, 0],
                [0, 0, 255],
                [255, 0, 255],
                [0, 255, 255],
                [255, 255, 255],
            ];
            base[n as usize]
        }
        16..=231 => {
            let i = n - 16;
            let b = i % 6;
            let g = (i / 6) % 6;
            let r = i / 36;
            let val = |x: u8| if x == 0 { 0 } else { x * 40 + 55 };
            [val(r), val(g), val(b)]
        }
        232..=255 => {
            let gray = (n - 232) * 10 + 8;
            [gray, gray, gray]
        }
    }
}

impl Request {
    fn with_id(self, id: &str) -> Request {
        match self {
            Request::Enumerate { .. } => Request::Enumerate { id: id.into() },
            Request::Create {
                session, window, ..
            } => Request::Create {
                id: id.into(),
                session,
                window,
            },
            Request::Attach { session, .. } => Request::Attach {
                id: id.into(),
                session,
            },
            Request::Rename {
                session,
                name,
                window,
                note,
                ..
            } => Request::Rename {
                id: id.into(),
                session,
                name,
                window,
                note,
            },
            Request::Destroy { session, .. } => Request::Destroy {
                id: id.into(),
                session,
            },
            Request::Read { session, pane, .. } => Request::Read {
                id: id.into(),
                session,
                pane,
            },
            Request::Split { window, rows, .. } => Request::Split {
                id: id.into(),
                window,
                rows,
            },
            Request::Focus { window, pane, .. } => Request::Focus {
                id: id.into(),
                window,
                pane,
            },
            Request::Close { window, pane, .. } => Request::Close {
                id: id.into(),
                window,
                pane,
            },
            Request::Resize { cols, rows, .. } => Request::Resize {
                id: id.into(),
                cols,
                rows,
            },
            Request::Spawn {
                pane,
                program,
                acp,
                watch,
                name,
                cwd,
                ..
            } => Request::Spawn {
                id: id.into(),
                pane,
                program,
                acp,
                watch,
                name,
                cwd,
            },
            Request::Write {
                data, pane, prompt, ..
            } => Request::Write {
                id: id.into(),
                data,
                pane,
                prompt,
            },
        }
    }
}

/// The client seat: attach, draw, forward keys. Prefix `q` detaches.
pub fn run(sock: &Path) -> io::Result<()> {
    use ratatui::crossterm::event::{
        DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
    };
    use ratatui::crossterm::execute;
    let mut client = Client::connect(sock)?;
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture, EnableFocusChange);
    let out = run_loop(&mut client, &mut terminal);
    let _ = execute!(std::io::stdout(), DisableMouseCapture, DisableFocusChange);
    ratatui::restore();
    out
}

/// An error owns the footer only when no name draft is live.
/// Otherwise the prompt stays visible and the error sits next to it.
fn error_owns_footer(naming: Option<&Naming>, last_error: Option<&str>) -> bool {
    last_error.is_some() && naming.is_none()
}

const PICK_ROW: u16 = 2;

fn agent_clause(agent: &Agent) -> String {
    let seats = agent.seats();
    if seats == [Seat::Anvil] {
        return "anvil".into();
    }
    if seats.contains(&Seat::Anvil) {
        return format!("{} · anvil", agent.program);
    }
    agent.program.clone()
}

fn pick_box(area: Rect, n: usize, min_w: u16, max_w: u16) -> Rect {
    let max_h = area.height.saturating_sub(6).max(6);
    let inner_h = ((n as u16).saturating_mul(PICK_ROW) + 1).min(max_h.saturating_sub(2));
    let height = inner_h.saturating_add(2).min(max_h);
    let width = (area.width * 2 / 3).clamp(min_w, max_w);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn pick_inner(popup: Rect) -> Rect {
    Rect {
        x: popup.x + 1,
        y: popup.y + 1,
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    }
}

fn pick_contains(popup: Rect, col: u16, row: u16) -> bool {
    col >= popup.x
        && col < popup.x.saturating_add(popup.width)
        && row >= popup.y
        && row < popup.y.saturating_add(popup.height)
}

fn pick_row(inner: Rect, n: usize, sel: usize, row: u16) -> Option<usize> {
    if n == 0 || row < inner.y || row >= inner.bottom() {
        return None;
    }
    let visible = (inner.height / PICK_ROW).max(1) as usize;
    let scroll = sel.saturating_sub(visible.saturating_sub(1));
    let i = ((row - inner.y) / PICK_ROW) as usize + scroll;
    (i < n && i < scroll + visible).then_some(i)
}

fn step_pick(idx: usize, n: usize, down: bool) -> usize {
    if n == 0 {
        return idx;
    }
    if down {
        (idx + 1) % n
    } else {
        (idx + n - 1) % n
    }
}

fn run_loop(
    client: &mut Client,
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) -> io::Result<()> {
    use ratatui::crossterm::event;
    let size = terminal.size()?;
    client.resize_tty(size.width, size.height)?;
    client.refresh()?;
    loop {
        client.bump_tick();
        terminal.draw(|frame| client.draw(frame))?;
        if !event::poll(Duration::from_millis(50))? {
            let _ = client.refresh();
            continue;
        }
        match event::read()? {
            event::Event::Key(key) => {
                if let Err(err) = client.key(key) {
                    client.last_error = Some(err.to_string());
                }
                if client.detached {
                    return Ok(());
                }
            }
            event::Event::Resize(cols, rows) => {
                if let Err(err) = client.resize_tty(cols, rows) {
                    client.last_error = Some(err.to_string());
                }
            }
            event::Event::Mouse(m) => {
                client.mouse(m)?;
            }
            event::Event::FocusGained => client.term_focused = true,
            event::Event::FocusLost => client.term_focused = false,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_hides_chrome_on_a_narrow_tty() {
        assert_eq!(canvas(79, 24, false, 21), (79, 22));
        assert_eq!(canvas(79, 24, true, 21), (79, 22));
    }

    #[test]
    fn canvas_rail_is_the_closed_sidebar() {
        assert_eq!(canvas(80, 24, false, 21), (76, 22));
        assert_eq!(canvas(117, 24, false, 21), (113, 22));
    }

    #[test]
    fn canvas_sidebar_opens_at_half_width() {
        assert_eq!(canvas(80, 24, true, 21), (58, 22));
        assert_eq!(canvas(117, 24, true, 21), (95, 22));
    }

    #[test]
    fn legacy_main_is_shown_as_an_index() {
        assert!(is_legacy_session("main"));
        assert!(is_legacy_session("main-2"));
        assert!(!is_legacy_session("1"));
        assert!(!is_legacy_session("personal"));
    }

    #[test]
    fn side_kind_follows_width_and_the_toggle() {
        assert_eq!(side(79, true), Side::Hidden);
        assert_eq!(side(80, false), Side::Rail);
        assert_eq!(side(80, true), Side::Open);
        assert_eq!(side(117, false), Side::Rail);
        assert_eq!(side(117, true), Side::Open);
    }

    fn sample_view() -> SessionView {
        SessionView {
            focused: "1".into(),
            windows: vec![
                crate::daemon::session::WindowView {
                    window: "oc".into(),
                    state: WindowState::Idle,
                    note: String::new(),
                    panes: vec![crate::daemon::session::PaneView {
                        pane: "1".into(),
                        x: 0,
                        y: 0,
                        cols: 40,
                        rows: 20,
                        name: Some("oc".into()),
                        activity: None,
                        session: None,
                        cwd: None,
                        state: WindowState::Idle,
                    }],
                },
                crate::daemon::session::WindowView {
                    window: "grok".into(),
                    state: WindowState::Idle,
                    note: String::new(),
                    panes: vec![crate::daemon::session::PaneView {
                        pane: "2".into(),
                        x: 0,
                        y: 0,
                        cols: 40,
                        rows: 20,
                        name: None,
                        activity: None,
                        session: None,
                        cwd: None,
                        state: WindowState::Idle,
                    }],
                },
            ],
        }
    }

    #[test]
    fn agent_clause_marks_anvil() {
        let oc = Agent {
            name: "oc".into(),
            program: "oc".into(),
            watch: Some("http".into()),
            acp_only: false,
            acp_program: Some("oc acp".into()),
            ..Default::default()
        };
        assert_eq!(agent_clause(&oc), "oc · anvil");
        let rung = Agent {
            name: "rung".into(),
            program: "rung-agent --acp".into(),
            watch: None,
            acp_only: true,
            acp_program: None,
            ..Default::default()
        };
        assert_eq!(agent_clause(&rung), "anvil");
    }

    #[test]
    fn pick_row_hits_the_clause_line_too() {
        let area = Rect::new(0, 0, 80, 24);
        let popup = pick_box(area, 3, 28, 48);
        let inner = pick_inner(popup);
        let first = inner.y;
        assert_eq!(pick_row(inner, 3, 0, first), Some(0));
        assert_eq!(pick_row(inner, 3, 0, first + 1), Some(0));
        assert_eq!(pick_row(inner, 3, 0, first + 2), Some(1));
        assert!(!pick_contains(popup, 0, 0));
        assert!(pick_contains(popup, popup.x + 1, popup.y + 1));
        assert_eq!(step_pick(0, 3, true), 1);
        assert_eq!(step_pick(0, 3, false), 2);
    }

    #[test]
    fn click_on_the_rail_selects_a_window() {
        let view = sample_view();
        assert_eq!(
            hit((120, 24), false, 21, 0.5, 1, 4, &view, &HashMap::new()),
            Some(Hit::Window("oc".into()))
        );
        assert_eq!(
            hit((120, 24), true, 21, 0.5, 1, 4, &view, &HashMap::new()),
            Some(Hit::Window("oc".into()))
        );
        assert_eq!(
            hit((120, 24), true, 21, 0.5, 1, 6, &view, &HashMap::new()),
            Some(Hit::Window("grok".into()))
        );
        assert_eq!(
            hit((120, 24), false, 21, 0.5, 1, 23, &view, &HashMap::new()),
            None
        );
        assert_eq!(
            hit((120, 24), false, 21, 0.5, 1, 0, &view, &HashMap::new()),
            None
        );
    }

    #[test]
    fn a_name_collision_keeps_the_prompt() {
        assert!(error_owns_footer(
            None,
            Some("a session by that name already exists")
        ));
        assert!(!error_owns_footer(
            Some(&Naming::Session("spire".into())),
            Some("a session by that name already exists"),
        ));
    }

    #[test]
    fn click_on_a_tile_selects_the_pane() {
        let view = sample_view();
        // rail: 3 + gap 1 = content starts at col 4
        assert_eq!(
            hit((120, 24), false, 21, 0.5, 4, 1, &view, &HashMap::new()),
            Some(Hit::Pane("1".into()))
        );
    }

    #[test]
    fn esc_is_passthrough_not_detach() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            encode_passthrough(&key, (0, false)).as_deref(),
            Some("\x1b")
        );
    }

    #[test]
    fn ctrl_c_is_etx_not_the_letter() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(
            encode_passthrough(&key, (0, false)).as_deref(),
            Some("\x03")
        );
        let plain = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        assert_eq!(encode_passthrough(&plain, (0, false)).as_deref(), Some("c"));
    }

    #[test]
    fn shift_enter_is_csi_u_when_the_pane_asked() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(encode_passthrough(&key, (0, false)).as_deref(), Some("\r"));
        assert_eq!(
            encode_passthrough(&key, (1, false)).as_deref(),
            Some("\x1b[13;2u")
        );
        assert_eq!(
            encode_passthrough(&key, (0, true)).as_deref(),
            Some("\x1b[27;2;13~")
        );
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            encode_passthrough(&enter, (1, false)).as_deref(),
            Some("\r")
        );
    }

    #[test]
    fn shift_arrows_keep_the_modifier() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT);
        assert_eq!(
            encode_passthrough(&key, (0, false)).as_deref(),
            Some("\x1b[1;2C")
        );
    }

    #[test]
    fn mouse_reaches_the_pane_only_when_it_asked() {
        let off = Grid {
            cols: 8,
            rows: 2,
            cursor_col: 0,
            cursor_row: 0,
            lines: vec!["        ".into(); 2],
            runs: vec![],
            alive: true,
            acp: false,
            mouse: false,
            kitty: 0,
            modify: false,
        };
        let mut on = off.clone();
        on.mouse = true;
        assert!(!mouse_for_pane(Some(&off)));
        assert!(mouse_for_pane(Some(&on)));
        assert!(!mouse_for_pane(None));
    }

    #[test]
    fn sgr_mouse_encodes_click_and_wheel() {
        use ratatui::crossterm::event::{MouseButton, MouseEventKind};
        assert_eq!(
            sgr_mouse(MouseEventKind::Down(MouseButton::Left), 3, 5).as_deref(),
            Some("\x1b[<0;3;5M")
        );
        assert_eq!(
            sgr_mouse(MouseEventKind::Up(MouseButton::Left), 3, 5).as_deref(),
            Some("\x1b[<0;3;5m")
        );
        assert_eq!(
            sgr_mouse(MouseEventKind::ScrollDown, 1, 2).as_deref(),
            Some("\x1b[<65;1;2M")
        );
    }

    #[test]
    fn tile_at_is_pane_local() {
        let view = sample_view();
        let t = tile_at((120, 24), false, 21, 6, 3, &view).unwrap();
        assert_eq!(t.pane, "1");
        assert_eq!(t.x, 3);
        assert_eq!(t.y, 3);
    }
}
