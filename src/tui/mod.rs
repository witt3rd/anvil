//! The client: views a session and sends keys. Immediate-mode chrome
//! over the daemon's character grids. The palette is the opencode
//! builtin theme (opaline) — this module names semantic tokens only,
//! never colors of its own.

pub mod keymap;

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use opaline::Theme;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui_which_key::WhichKey;

use crate::daemon::pane::Grid;
use crate::daemon::session::SessionView;
use crate::proto::{Reply, Request, Value};

use keymap::{Action, AppWhichKey, Scope, build_which_key_state};

/// The opencode palette, shipped with anvil and loaded through
/// opaline's public loader — opaline itself stays untouched.
const THEME_TOML: &str = include_str!("../../themes/opencode.toml");

// The chrome geometry.
const RAIL_COLS: u16 = 3; // mark column
const ROSTER_COLS: u16 = 42; // opened activity list
const RAIL_MIN: u16 = 80; // rail shows from this width on
const ROSTER_MIN: u16 = 120; // roster needs this width
const PAD: u16 = 2; // the content gutter on each side
const GAP: u16 = 1; // between the sidebar and the content
const STATUS_LINES: u16 = 1; // the status line height
const MARK_IDLE: &str = "◇";
const MARK_DEAD: &str = "◇";

/// How the left chrome is drawn for this tty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Hidden,
    Rail,
    Roster,
}

fn side(term_w: u16, roster_open: bool) -> Side {
    if roster_open && term_w >= ROSTER_MIN {
        Side::Roster
    } else if term_w >= RAIL_MIN {
        Side::Rail
    } else {
        Side::Hidden
    }
}

fn side_width(side: Side) -> u16 {
    match side {
        Side::Hidden => 0,
        Side::Rail => RAIL_COLS,
        Side::Roster => ROSTER_COLS,
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
/// chrome. The rail shows from `RAIL_MIN`; the roster from `ROSTER_MIN`
/// when it is open.
fn canvas(term_w: u16, term_h: u16, roster_open: bool) -> (u16, u16) {
    let chrome = match side(term_w, roster_open) {
        Side::Hidden => 2 * PAD,
        other => side_width(other) + GAP + 2 * PAD,
    };
    (term_w.saturating_sub(chrome), term_h.saturating_sub(STATUS_LINES))
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
    roster_open: bool,
    tty: (u16, u16),
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
            roster_open: false,
            tty: (80, 24),
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

    fn add_window(&mut self) -> io::Result<()> {
        let session = self.attached.clone().ok_or_else(|| io::Error::other("no session"))?;
        self.call(Request::Create { id: String::new(), session, window: Some("1".into()) })?;
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
        self.call(Request::Spawn { id: String::new(), pane: pane.into(), program: self.shell.clone() })?;
        Ok(())
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
        let name = sessions.first().cloned().unwrap_or_else(|| "main".to_string());
        if sessions.is_empty() {
            self.create(&name)?;
        }
        self.attach(&name)?;
        self.refresh()
    }

    /// A new session, and attach to it.
    pub fn new_session(&mut self) -> io::Result<()> {
        let name = {
            let mut n = self.sessions.len() + 1;
            loop {
                let candidate = format!("main-{n}");
                if !self.sessions.contains(&candidate) {
                    break candidate;
                }
                n += 1;
            }
        };
        self.create(&name)?;
        self.attach(&name)?;
        self.refresh()
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
        self.refresh()
    }

    /// The tty changed size: the session relays out to the canvas.
    pub fn resize_tty(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        self.tty = (cols, rows);
        let (cols, rows) = canvas(cols, rows, self.roster_open);
        self.resize(cols.max(2), rows.max(2))
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
            if !grid.alive {
                let _ = self.spawn(&id);
            }
            self.grids.insert(id, grid);
        }
        self.view = Some(view);
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
    /// has ended is a normal state, not a client error: the client
    /// reads the panes again — which respawns the dead ones — and the
    /// key is dropped.
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
            Action::ToggleRoster => {
                self.roster_open = !self.roster_open;
                let (w, h) = self.tty;
                self.resize_tty(w, h)?;
                self.refresh()
            }
            Action::NewSession => self.new_session(),
            Action::SwitchSession(n) => self.switch_session(n),
            Action::NewWindow => {
                self.add_window()?;
                self.refresh()
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

    /// Draw one frame: fill the base, the rail or roster, the panes'
    /// grids, the status line, and the prefix popup. The frame and the
    /// tiles share `bg.base`, so the gap between tiles is invisible —
    /// a single thin separator line marks the boundary.
    pub fn draw(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        frame.render_widget(Block::default().bg(self.c("bg.base")), area);

        match side(area.width, self.roster_open) {
            Side::Hidden => self.draw_content(frame, area),
            kind => {
                let chunks = Layout::horizontal([
                    Constraint::Length(side_width(kind)),
                    Constraint::Length(GAP),
                    Constraint::Fill(1),
                ])
                .split(area);
                self.draw_side(frame, chunks[0], kind);
                self.draw_content(frame, chunks[2]);
            }
        }
        self.draw_status(frame, area);

        if self.which_key.active || !self.which_key.current_sequence.is_empty() {
            let popup = WhichKey::new().border_style(Style::default().fg(self.c("border.focused")));
            popup.render(frame.buffer_mut(), &self.which_key);
        }
    }

    /// The rail or the roster: windows of the attached session, in
    /// the order the operator laid them. The mark is idle or dead
    /// until ACP feeds turning / needs-you.
    fn draw_side(&self, frame: &mut ratatui::Frame, area: Rect, kind: Side) {
        let bg = match kind {
            Side::Roster => "bg.panel",
            _ => "bg.base",
        };
        frame.render_widget(Block::default().bg(self.c(bg)), area);
        let Some(view) = &self.view else {
            return;
        };
        let current = self.focused_window();
        let accent = self.c("accent.primary");
        let muted = self.c("text.muted");
        let primary = self.c("text.primary");
        for (y, window) in (area.y..area.bottom()).zip(view.windows.iter()) {
            let here = current.as_deref() == Some(window.window.as_str());
            let alive = self.window_alive(window);
            let mark = if alive { MARK_IDLE } else { MARK_DEAD };
            let mark_style = Style::default().fg(muted);
            if here {
                frame.buffer_mut().set_stringn(area.x, y, "┃", 1, Style::default().fg(accent));
            }
            frame.buffer_mut().set_stringn(
                area.x + 2,
                y,
                mark,
                1,
                mark_style,
            );
            if kind == Side::Roster {
                let name_style = if here {
                    Style::default().fg(primary)
                } else {
                    Style::default().fg(muted)
                };
                let clause = if alive { "idle" } else { "dead" };
                let line = format!("{}  {clause}", window.window);
                frame.buffer_mut().set_stringn(
                    area.x + 4,
                    y,
                    &line,
                    area.width.saturating_sub(4) as usize,
                    name_style,
                );
            }
        }
    }

    /// A window is alive when any of its panes still has a process.
    fn window_alive(&self, window: &crate::daemon::session::WindowView) -> bool {
        window.panes.iter().any(|p| {
            self.grids.get(&p.pane).is_none_or(|g| g.alive)
        })
    }

    /// The content: each pane's retained grid at its geometry. Panes
    /// with no process show a blank panel.
    fn draw_content(&self, frame: &mut ratatui::Frame, area: Rect) {
        let inner = Rect {
            x: area.x + PAD,
            y: area.y,
            width: area.width.saturating_sub(2 * PAD),
            height: area.height.saturating_sub(STATUS_LINES),
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

    /// The status line: the session and its focused pane on the left,
    /// the key hints on the right.
    fn draw_status(&self, frame: &mut ratatui::Frame, area: Rect) {
        let y = area.bottom().saturating_sub(1);
        let left = match &self.attached {
            Some(name) => {
                let pane = self.view.as_ref().map(|v| v.focused.clone());
                match pane {
                    Some(pane) => format!("{name} · {pane}"),
                    None => name.clone(),
                }
            }
            None => "anvil".to_string(),
        };
        frame.buffer_mut().set_stringn(
            area.x + PAD,
            y,
            &left,
            (area.width.saturating_sub(2 * PAD)) as usize,
            Style::default().fg(self.c("text.primary")),
        );
        let hints = "ctrl-b prefix · s roster · esc detach";
        let hints_w = hints.len() as u16;
        if hints_w + 2 * PAD <= area.width {
            frame.buffer_mut().set_stringn(
                area.right().saturating_sub(hints_w + PAD),
                y,
                hints,
                hints_w as usize,
                Style::default().fg(self.c("text.muted")),
            );
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
            Request::Rename { session, name, .. } => Request::Rename {
                id: id.into(),
                session,
                name,
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
            Request::Spawn { pane, program, .. } => Request::Spawn {
                id: id.into(),
                pane,
                program,
            },
            Request::Write { data, .. } => Request::Write { id: id.into(), data },
        }
    }
}

/// The client seat: attach, draw, forward keys. `esc` detaches.
pub fn run(sock: &Path) -> io::Result<()> {
    let mut client = Client::connect(sock)?;
    let mut terminal = ratatui::init();
    let out = run_loop(&mut client, &mut terminal);
    ratatui::restore();
    out
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
        terminal.draw(|frame| client.draw(frame))?;
        if !event::poll(Duration::from_millis(50))? {
            let _ = client.refresh();
            continue;
        }
        match event::read()? {
            event::Event::Key(key) => {
                client.key(key)?;
                if client.detached {
                    return Ok(());
                }
            }
            event::Event::Resize(cols, rows) => {
                client.resize_tty(cols, rows)?;
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
        assert_eq!(canvas(79, 24, false), (75, 23));
        assert_eq!(canvas(79, 24, true), (75, 23));
    }

    #[test]
    fn canvas_rail_is_the_rest_state() {
        assert_eq!(canvas(80, 24, false), (72, 23));
        assert_eq!(canvas(119, 24, true), (111, 23));
    }

    #[test]
    fn canvas_roster_opens_on_a_wide_tty() {
        assert_eq!(canvas(120, 24, false), (112, 23));
        assert_eq!(canvas(120, 24, true), (73, 23));
    }

    #[test]
    fn side_kind_follows_width_and_the_toggle() {
        assert_eq!(side(79, true), Side::Hidden);
        assert_eq!(side(80, false), Side::Rail);
        assert_eq!(side(120, false), Side::Rail);
        assert_eq!(side(120, true), Side::Roster);
    }
}