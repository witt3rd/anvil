//! The client: views a session and sends keys. Immediate-mode chrome
//! over the daemon's character grids. The palette is the opencode
//! builtin theme (opaline) — this module names semantic tokens only,
//! never colors of its own.

pub mod agents;
pub mod keymap;
pub mod sat;
pub mod sessions;
pub mod side;

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use opaline::Theme;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui_which_key::WhichKey;

use crate::daemon::acp::WindowState;
use crate::daemon::pane::Grid;
use crate::daemon::session::SessionView;
use crate::proto::{Reply, Request, Value};

use agents::{Agent, Agents, unique_name};
use keymap::{Action, AppWhichKey, Scope, build_which_key_state};

/// The opencode palette, shipped with anvil and loaded through
/// opaline's public loader — opaline itself stays untouched.
const THEME_TOML: &str = include_str!("../../themes/opencode.toml");

// The chrome geometry.
const RAIL_COLS: u16 = 3; // mark column
const RAIL_MIN: u16 = 80; // rail shows from this width on
const SIDE_MIN: u16 = 80; // open sidebar on the same tty the rail does
const PAD: u16 = 2; // the content gutter on each side
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
        Side::Hidden => 2 * PAD,
        other => side_width(other, cols) + GAP + 2 * PAD,
    };
    (term_w.saturating_sub(chrome), term_h.saturating_sub(CHROME_ROWS))
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
    sessions_pick: Option<usize>,
    session_rows: Vec<sessions::Row>,
    host: String,
    sat: crate::daemon::sat::Snap,
}

enum Drag {
    Width,
    Split,
}

/// What the name draft is for.
enum Naming {
    Window(String),
    Session(String),
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
            sessions_pick: None,
            session_rows: Vec::new(),
            host: host_name(),
            sat: crate::daemon::sat::Snap::load(&agents::default_root()),
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
                    eprintln!("anvil client: bad reply (first 400 chars): {:?}", &reply[..reply.len().min(400)]);
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
            reply.value.ok_or_else(|| io::Error::other("the daemon replied without a value"))
        } else {
            Err(io::Error::other(reply.error.unwrap_or_else(|| "the daemon refused".into())))
        }
    }

    fn enumerate(&mut self) -> io::Result<Vec<String>> {
        match self.call(Request::Enumerate { id: String::new() })? {
            Value::Sessions { sessions } => {
                self.sessions = sessions.clone();
                Ok(sessions)
            }
            _ => Err(io::Error::other("enumerate replied with the wrong shape")),
        }
    }

    fn create(&mut self, name: &str) -> io::Result<()> {
        self.call(Request::Create { id: String::new(), session: name.into(), window: None })?;
        Ok(())
    }

    fn attach(&mut self, name: &str) -> io::Result<()> {
        self.call(Request::Attach { id: String::new(), session: name.into() })?;
        self.attached = Some(name.into());
        Ok(())
    }

    fn add_window(&mut self, name: &str) -> io::Result<()> {
        let session = self.attached.clone().ok_or_else(|| io::Error::other("no session"))?;
        self.call(Request::Create {
            id: String::new(),
            session,
            window: Some(name.into()),
        })?;
        Ok(())
    }

    fn rename_window(&mut self, name: &str) -> io::Result<()> {
        let session = self.attached.clone().ok_or_else(|| io::Error::other("no session"))?;
        let window = self
            .focused_window()
            .ok_or_else(|| io::Error::other("no window"))?;
        self.call(Request::Rename {
            id: String::new(),
            session,
            name: name.into(),
            window: Some(window),
        })?;
        Ok(())
    }

    fn rename_session(&mut self, name: &str) -> io::Result<()> {
        let session = self.attached.clone().ok_or_else(|| io::Error::other("no session"))?;
        self.call(Request::Rename {
            id: String::new(),
            session,
            name: name.into(),
            window: None,
        })?;
        self.attached = Some(name.into());
        self.enumerate()?;
        Ok(())
    }

    fn split(&mut self, window: &str) -> io::Result<()> {
        self.call(Request::Split { id: String::new(), window: window.into() })?;
        Ok(())
    }

    fn read_view(&mut self) -> io::Result<SessionView> {
        match self.call(Request::Read { id: String::new(), session: self.attached.clone(), pane: None })? {
            Value::View(view) => Ok(view),
            _ => Err(io::Error::other("read replied with the wrong shape")),
        }
    }

    fn read_pane(&mut self, pane: &str) -> io::Result<Grid> {
        match self.call(Request::Read { id: String::new(), session: None, pane: Some(pane.into()) })? {
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
        })?;
        Ok(())
    }

    fn spawn_tui(
        &mut self,
        pane: &str,
        program: &str,
        watch: Option<String>,
        name: &str,
    ) -> io::Result<()> {
        self.call(Request::Spawn {
            id: String::new(),
            pane: pane.into(),
            program: program.into(),
            acp: false,
            watch,
            name: Some(name.into()),
        })?;
        Ok(())
    }

    fn window_names(&self) -> Vec<String> {
        self.view
            .as_ref()
            .map(|v| v.windows.iter().map(|w| w.window.clone()).collect())
            .unwrap_or_default()
    }

    /// A new window running the agent's TUI. The daemon watches the
    /// HTTP door for the rail.
    fn launch_agent(&mut self, agent: &Agent) -> io::Result<()> {
        let (program, watch) = agent.tui_spawn();
        let name = unique_name(&agent.name, &self.window_names());
        self.add_window(&name)?;
        self.refresh()?;
        let pane = self.view.as_ref().map(|v| v.focused.clone());
        if let Some(pane) = pane {
            match self.spawn_tui(&pane, &program, watch, &agent.name) {
                Ok(()) => self.last_error = None,
                Err(err) => self.last_error = Some(err.to_string()),
            }
        }
        self.refresh()
    }

    /// A new window running a shell.
    fn launch_terminal(&mut self) -> io::Result<()> {
        let name = unique_name("sh", &self.window_names());
        self.add_window(&name)?;
        self.refresh()
    }

    fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        self.call(Request::Resize { id: String::new(), cols, rows })?;
        Ok(())
    }

    fn write(&mut self, data: &str) -> io::Result<()> {
        self.call(Request::Write { id: String::new(), data: data.into() })?;
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
        let name = self
            .sessions
            .first()
            .cloned()
            .unwrap_or(name);
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
        self.view = Some(view);
        self.sat = crate::daemon::sat::Snap::load(&agents::default_root());
        Ok(())
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
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&dbg)?;
                writeln!(f, "key: code={:?} mods={:?} active={}", key.code, key.modifiers, self.which_key.active)
            })();
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
                    if n > 0 {
                        self.sessions_pick = Some((idx + 1) % n);
                    }
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if n > 0 {
                        self.sessions_pick = Some((idx + n - 1) % n);
                    }
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
                        return self.launch_agent(&agent);
                    }
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if n > 0 {
                        self.picking = Some((idx + 1) % n);
                    }
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if n > 0 {
                        self.picking = Some((idx + n - 1) % n);
                    }
                    return Ok(());
                }
                KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                    let i = (c as u8 - b'1') as usize;
                    if i < n {
                        self.picking = None;
                        if let Some(agent) = self.catalog.agents.get(i).cloned() {
                            return self.launch_agent(&agent);
                        }
                    }
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
                || (key.code == KeyCode::Char('4')
                    && key.modifiers.contains(KeyModifiers::SHIFT))
            {
                return self.dispatch(Action::RenameSession);
            }
            // `&` is prefix-& (tmux kill-window). Terminals send
            // Char('&') or Shift-7.
            if matches!(key.code, KeyCode::Char('&'))
                || (key.code == KeyCode::Char('7')
                    && key.modifiers.contains(KeyModifiers::SHIFT))
            {
                return self.dispatch(Action::CloseWindow);
            }
            if let Some(action) = self.which_key.handle_key(key) {
                return self.dispatch(action);
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => {
                self.detached = true;
                return Ok(());
            }
            KeyCode::Enter => self.forward("\r"),
            KeyCode::Backspace => self.forward("\u{7f}"),
            KeyCode::Tab => self.forward("\t"),
            KeyCode::Up => self.forward("\x1b[A"),
            KeyCode::Down => self.forward("\x1b[B"),
            KeyCode::Right => self.forward("\x1b[C"),
            KeyCode::Left => self.forward("\x1b[D"),
            KeyCode::Char(c) => {
                let mut buf = [0u8; 4];
                self.forward(c.encode_utf8(&mut buf));
            }
            _ => {}
        }
        Ok(())
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
                self.launch_agent(&agent)
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
                let window = self.focused_window();
                if let Some(window) = window {
                    self.split(&window)?;
                }
                self.refresh()
            }
            Action::FocusLeft => self.focus_dir(FocusDir::Left),
            Action::FocusRight => self.focus_dir(FocusDir::Right),
            Action::FocusUp => self.focus_dir(FocusDir::Up),
            Action::FocusDown => self.focus_dir(FocusDir::Down),
            Action::ClosePane => {
                if let Some(pane) = self.view.as_ref().map(|v| v.focused.clone()) {
                    self.call(Request::Close { id: String::new(), window: None, pane: Some(pane) })?;
                }
                self.refresh()
            }
            Action::CloseWindow => {
                if let Some(window) = self.focused_window() {
                    self.call(Request::Close { id: String::new(), window: Some(window), pane: None })?;
                }
                self.refresh()
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
        self.call(Request::Focus { id: String::new(), window: None, pane: Some(target) })?;
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
        let view = self.view.as_ref().ok_or_else(|| io::Error::other("no session"))?;
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
        self.call(Request::Focus { id: String::new(), window: Some(window), pane: None })?;
        Ok(())
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
        if self.drag.is_some() {
            return match kind {
                MouseEventKind::Drag(_) => self.mouse_drag(col, row),
                MouseEventKind::Up(_) => self.mouse_up(),
                _ => Ok(()),
            };
        }
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.chrome_down(col, row)? {
                    return Ok(());
                }
                self.mouse_to_tile(col, row, kind)
            }
            MouseEventKind::Drag(_)
            | MouseEventKind::Up(_)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
            | MouseEventKind::Down(_) => self.mouse_to_tile(col, row, kind),
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
            let lay = side::layout(area, kind == Side::Open, self.sidebar.split, view);
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

    /// Focus the tile under the cursor and write an SGR mouse
    /// sequence into that pane, so the inner TUI can select and scroll.
    fn mouse_to_tile(
        &mut self,
        col: u16,
        row: u16,
        kind: ratatui::crossterm::event::MouseEventKind,
    ) -> io::Result<()> {
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
        let Some(seq) = sgr_mouse(kind, tile.x, tile.y) else {
            return Ok(());
        };
        if self.view.as_ref().is_some_and(|v| v.focused != tile.pane) {
            let pane = tile.pane.clone();
            self.call(Request::Focus {
                id: String::new(),
                window: None,
                pane: Some(pane),
            })?;
            self.refresh()?;
        }
        self.write(&seq)
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

        if self.which_key.active || !self.which_key.current_sequence.is_empty() {
            let popup = WhichKey::new().border_style(Style::default().fg(self.c("border.focused")));
            popup.render(frame.buffer_mut(), &self.which_key);
        }
    }

    /// Windows above, agent processes below. The open sidebar writes
    /// two lines per entry: the name, then a clause.
    fn draw_side(&self, frame: &mut ratatui::Frame, area: Rect, kind: Side) {
        let open = kind == Side::Open;
        let bg = if open { "bg.panel" } else { "bg.base" };
        frame.render_widget(Block::default().bg(self.c(bg)), area);
        let Some(view) = &self.view else {
            return;
        };
        let lay = side::layout(area, open, self.sidebar.split, view);
        let label = Style::default().fg(self.c("text.dim"));
        let border = Style::default().fg(self.c("border.subtle"));
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
        if open {
            if let Some(counters) = self.sat_session() {
                let strip = Rect {
                    x: lay.agent_area.x,
                    y: lay.agent_area.y,
                    width: 1,
                    height: lay.agent_area.height,
                };
                let fill = Style::default().fg(self.c(sat::fill_token(counters.agents)));
                sat::draw_strip(
                    frame.buffer_mut(),
                    strip,
                    &counters,
                    Style::default().fg(self.c(sat::track_token())),
                    fill,
                    Style::default().fg(self.c(sat::stain_token())),
                );
            }
        }
        if let Some(y) = lay.divider_y {
            let grip = matches!(self.drag, Some(Drag::Split));
            let style = if grip { Style::default().fg(self.c("accent.primary")) } else { border };
            let line = "─".repeat(area.width.max(1) as usize);
            frame
                .buffer_mut()
                .set_stringn(area.x, y, &line, area.width as usize, style);
        }
        for (y, h, item) in &lay.hits {
            self.draw_side_item(frame, area, *y, *h, item, open, view);
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
            side::SideItem::Agent { pane, window, name } => {
                let here = focused_pane == pane;
                let state = view
                    .windows
                    .iter()
                    .flat_map(|w| w.panes.iter())
                    .find(|p| p.pane == *pane)
                    .map(|p| p.state)
                    .unwrap_or(WindowState::Idle);
                let (mark, mark_style) = self.state_mark(state);
                (here, mark, mark_style, name.clone(), side::agent_clause(window, state))
            }
        };
        let accent = Style::default().fg(self.c("accent.primary"));
        let primary = Style::default().fg(self.c("text.primary"));
        let muted = Style::default().fg(self.c("text.muted"));
        let x = if open && matches!(item, side::SideItem::Agent { .. }) {
            area.x + 1
        } else {
            area.x
        };
        if here {
            frame.buffer_mut().set_stringn(x, y, "┃", 1, accent);
        }
        frame
            .buffer_mut()
            .set_stringn(x + 2, y, mark, 1, mark_style);
        if open {
            let title_style = if here { primary } else { muted };
            frame.buffer_mut().set_stringn(
                x + 4,
                y,
                &title,
                area.width.saturating_sub((x + 4).saturating_sub(area.x)) as usize,
                title_style,
            );
            if h > 1 && y + 1 < area.bottom() {
                frame.buffer_mut().set_stringn(
                    x + 4,
                    y + 1,
                    &clause,
                    area.width.saturating_sub((x + 4).saturating_sub(area.x)) as usize,
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
        let inner = Rect {
            x: area.x + PAD,
            y: area.y,
            width: area.width.saturating_sub(2 * PAD),
            height: area.height,
        };
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
    fn draw_separators(&self, frame: &mut ratatui::Frame, inner: Rect, panes: &[crate::daemon::session::PaneView]) {
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
            area.x.saturating_add(area.width / 2).saturating_sub(wordmark.len() as u16 / 2),
            y,
            wordmark,
            area.width as usize,
            Style::default().fg(self.c("accent.primary")),
        );
        frame.buffer_mut().set_stringn(
            area.x.saturating_add(area.width / 2).saturating_sub(subtitle.len() as u16 / 2),
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
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            rect,
        );
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
        frame.render_widget(Block::default().bg(self.c("bg.panel")), bar);
        let chip = format!(" {} ", self.session_label());
        frame.buffer_mut().set_stringn(
            area.x + PAD,
            bar.y,
            &chip,
            chip.len(),
            Style::default()
                .fg(self.c("bg.base"))
                .bg(self.c("accent.primary")),
        );
        let host = &self.host;
        let hx = area.right().saturating_sub(PAD + host.len() as u16);
        if hx + host.len() as u16 <= area.right() {
            frame.buffer_mut().set_stringn(
                hx,
                bar.y,
                host,
                host.len(),
                Style::default().fg(self.c("text.dim")),
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

    fn sat_session(&self) -> Option<crate::daemon::sat::Counters> {
        let view = self.view.as_ref()?;
        let (busy, agents) = crate::daemon::sat::count_view(view);
        if agents == 0 {
            return None;
        }
        let mut c = self
            .attached
            .as_ref()
            .and_then(|n| self.sat.session(n).cloned())
            .unwrap_or_default();
        c.busy = busy;
        c.agents = agents;
        Some(c)
    }

    fn paint_sat_header(
        &self,
        frame: &mut ratatui::Frame,
        track: Rect,
        counters: &crate::daemon::sat::Counters,
    ) {
        let fill = Style::default().fg(self.c(sat::fill_token(counters.agents)));
        sat::draw_header(
            frame.buffer_mut(),
            track,
            counters,
            Style::default().fg(self.c(sat::track_token())),
            fill,
            Style::default().fg(self.c(sat::stain_token())),
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
                ("enter", "switch"),
                ("n", "new"),
                ("x", "drop"),
                ("esc", "cancel"),
            ];
        }
        if self.picking.is_some() {
            return vec![
                ("j/k", "move"),
                ("enter", "launch"),
                ("d", "default"),
                ("esc", "cancel"),
            ];
        }
        if self.naming.is_some() {
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
        if self.window_count() > 1 {
            hints.push(("ctrl-b ]", "next"));
        }
        if self.pane_count() > 1 {
            hints.push(("ctrl-b h", "pane"));
        }
        if self.sessions.len() > 1 {
            hints.push(("ctrl-b s", "sessions"));
        }
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
        let asking = self.naming.is_some();
        let bar_bg = if asking { "error" } else { "bg.panel" };
        frame.render_widget(Block::default().bg(self.c(bar_bg)), bar);
        if error_owns_footer(self.naming.as_ref(), self.last_error.as_deref()) {
            if let Some(err) = &self.last_error {
                frame.buffer_mut().set_stringn(
                    area.x + PAD,
                    y,
                    err,
                    (area.width.saturating_sub(2 * PAD)) as usize,
                    Style::default().fg(self.c("error")),
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
                .add_modifier(Modifier::BOLD)
        };
        let desc = if asking {
            Style::default().fg(self.c("bg.base")).bg(self.c("error"))
        } else {
            Style::default().fg(self.c("text.dim"))
        };
        let pipe = if asking {
            Style::default().fg(self.c("bg.base")).bg(self.c("error"))
        } else {
            Style::default().fg(self.c("text.dim"))
        };
        let mut spans: Vec<Span> = Vec::new();
        if self.sessions_pick.is_some() {
            // The popup is the list; the footer is only keys.
        } else if let Some(idx) = self.picking {
            for (i, agent) in self.catalog.agents.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw("  "));
                }
                let style = if i == idx { key } else { desc };
                let label = if i == idx {
                    format!("[{}]", agent.name)
                } else {
                    agent.name.clone()
                };
                spans.push(Span::styled(label, style));
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
            spans.push(Span::styled(
                format!("rename {kind}: {draft}_"),
                key,
            ));
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

    fn draw_sessions_popup(&self, frame: &mut ratatui::Frame, area: Rect) {
        let Some(sel) = self.sessions_pick else {
            return;
        };
        let rows = &self.session_rows;
        let row_h: u16 = 2;
        let max_h = area.height.saturating_sub(6).max(6);
        let inner_h = ((rows.len() as u16).saturating_mul(row_h) + 1).min(max_h.saturating_sub(2));
        let height = inner_h.saturating_add(2).min(max_h);
        let width = (area.width * 2 / 3).clamp(36, 64);
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Block::default()
                .title(" sessions ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.c("border.focused")))
                .bg(self.c("bg.panel")),
            popup,
        );
        let inner = Rect {
            x: popup.x + 1,
            y: popup.y + 1,
            width: popup.width.saturating_sub(2),
            height: popup.height.saturating_sub(2),
        };
        let visible = (inner.height / row_h).max(1) as usize;
        let scroll = sel.saturating_sub(visible.saturating_sub(1));
        let current = self.attached.as_deref();
        for (n, row) in rows.iter().enumerate().skip(scroll).take(visible) {
            let y = inner.y + ((n - scroll) as u16) * row_h;
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
                        height: row_h.min(inner.bottom().saturating_sub(y)),
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
) -> Option<Hit> {
    let (tw, th) = tty;
    if row < HEADER_LINES || row >= th.saturating_sub(STATUS_LINES) {
        return None;
    }
    let kind = side(tw, open);
    let sw = side_width(kind, cols);
    if kind != Side::Hidden && col < sw {
        let area = Rect::new(0, HEADER_LINES, sw, th.saturating_sub(CHROME_ROWS));
        return match side::layout(area, kind == Side::Open, split, view).hit(row) {
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
    let content_x = if kind == Side::Hidden { 0 } else { sw + GAP };
    let inner_x = content_x + PAD;
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

/// xterm SGR mouse (`CSI < btn ; x ; y M/m`), 1-based, pane-local.
fn sgr_mouse(
    kind: ratatui::crossterm::event::MouseEventKind,
    x: u16,
    y: u16,
) -> Option<String> {
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
                [0, 0, 0], [128, 0, 0], [0, 128, 0], [128, 128, 0],
                [0, 0, 128], [128, 0, 128], [0, 128, 128], [192, 192, 192],
                [128, 128, 128], [255, 0, 0], [0, 255, 0], [255, 255, 0],
                [0, 0, 255], [255, 0, 255], [0, 255, 255], [255, 255, 255],
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
            Request::Create { session, window, .. } => Request::Create {
                id: id.into(),
                session,
                window,
            },
            Request::Attach { session, .. } => Request::Attach { id: id.into(), session },
            Request::Rename {
                session,
                name,
                window,
                ..
            } => Request::Rename {
                id: id.into(),
                session,
                name,
                window,
            },
            Request::Destroy { session, .. } => Request::Destroy { id: id.into(), session },
            Request::Read { session, pane, .. } => Request::Read {
                id: id.into(),
                session,
                pane,
            },
            Request::Split { window, .. } => Request::Split { id: id.into(), window },
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
            Request::Resize { cols, rows, .. } => Request::Resize { id: id.into(), cols, rows },
            Request::Spawn {
                pane,
                program,
                acp,
                watch,
                name,
                ..
            } => Request::Spawn {
                id: id.into(),
                pane,
                program,
                acp,
                watch,
                name,
            },
            Request::Write { data, .. } => Request::Write { id: id.into(), data },
        }
    }
}

/// The client seat: attach, draw, forward keys. `esc` detaches.
pub fn run(sock: &Path) -> io::Result<()> {
    use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
    use ratatui::crossterm::execute;
    let mut client = Client::connect(sock)?;
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let out = run_loop(&mut client, &mut terminal);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    out
}

/// An error owns the footer only when no name draft is live.
/// Otherwise the prompt stays visible and the error sits next to it.
fn error_owns_footer(naming: Option<&Naming>, last_error: Option<&str>) -> bool {
    last_error.is_some() && naming.is_none()
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
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_hides_chrome_on_a_narrow_tty() {
        assert_eq!(canvas(79, 24, false, 21), (75, 22));
        assert_eq!(canvas(79, 24, true, 21), (75, 22));
    }

    #[test]
    fn canvas_rail_is_the_closed_sidebar() {
        assert_eq!(canvas(80, 24, false, 21), (72, 22));
        assert_eq!(canvas(117, 24, false, 21), (109, 22));
    }

    #[test]
    fn canvas_sidebar_opens_at_half_width() {
        assert_eq!(canvas(80, 24, true, 21), (54, 22));
        assert_eq!(canvas(117, 24, true, 21), (91, 22));
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
                    panes: vec![crate::daemon::session::PaneView {
                        pane: "1".into(),
                        x: 0,
                        y: 0,
                        cols: 40,
                        rows: 20,
                        name: Some("oc".into()),
                        state: WindowState::Idle,
                    }],
                },
                crate::daemon::session::WindowView {
                    window: "grok".into(),
                    state: WindowState::Idle,
                    panes: vec![crate::daemon::session::PaneView {
                        pane: "2".into(),
                        x: 0,
                        y: 0,
                        cols: 40,
                        rows: 20,
                        name: None,
                        state: WindowState::Idle,
                    }],
                },
            ],
        }
    }

    #[test]
    fn click_on_the_rail_selects_a_window() {
        let view = sample_view();
        assert_eq!(
            hit((120, 24), false, 21, 0.5, 1, 4, &view),
            Some(Hit::Window("oc".into()))
        );
        assert_eq!(
            hit((120, 24), true, 21, 0.5, 1, 4, &view),
            Some(Hit::Window("oc".into()))
        );
        assert_eq!(
            hit((120, 24), true, 21, 0.5, 1, 6, &view),
            Some(Hit::Window("grok".into()))
        );
        assert_eq!(hit((120, 24), false, 21, 0.5, 1, 23, &view), None);
        assert_eq!(hit((120, 24), false, 21, 0.5, 1, 0, &view), None);
    }

    #[test]
    fn a_name_collision_keeps_the_prompt() {
        assert!(error_owns_footer(None, Some("a session by that name already exists")));
        assert!(!error_owns_footer(
            Some(&Naming::Session("spire".into())),
            Some("a session by that name already exists"),
        ));
    }

    #[test]
    fn click_on_a_tile_selects_the_pane() {
        let view = sample_view();
        // rail: 3 + gap 1 + pad 2 = content starts at col 6
        assert_eq!(
            hit((120, 24), false, 21, 0.5, 6, 1, &view),
            Some(Hit::Pane("1".into()))
        );
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
        let t = tile_at((120, 24), false, 21, 8, 3, &view).unwrap();
        assert_eq!(t.pane, "1");
        assert_eq!(t.x, 3);
        assert_eq!(t.y, 3);
    }
}