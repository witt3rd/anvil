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

// The chrome geometry, in opencode's proportions.
const SIDEBAR_COLS: u16 = 42; // the session list column
const WIDE_MIN: u16 = 120; // the sidebar shows from this width on
const PAD: u16 = 2; // the content gutter on each side
const GAP: u16 = 1; // between the sidebar and the content
const STATUS_LINES: u16 = 1; // the status line height

/// The pane canvas the session tty occupies: the terminal minus the
/// chrome. The sidebar hides below `WIDE_MIN`.
fn canvas(term_w: u16, term_h: u16) -> (u16, u16) {
    let wide = term_w >= WIDE_MIN;
    let side = if wide { SIDEBAR_COLS + GAP + 2 * PAD } else { 2 * PAD };
    (term_w.saturating_sub(side), term_h.saturating_sub(STATUS_LINES))
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
        let (cols, rows) = canvas(cols, rows);
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
            KeyCode::Enter => return self.write("\r"),
            KeyCode::Backspace => return self.write("\u{7f}"),
            KeyCode::Tab => return self.write("\t"),
            KeyCode::Up => return self.write("\x1b[A"),
            KeyCode::Down => return self.write("\x1b[B"),
            KeyCode::Right => return self.write("\x1b[C"),
            KeyCode::Left => return self.write("\x1b[D"),
            KeyCode::Char(c) => {
                let mut buf = [0u8; 4];
                self.write(c.encode_utf8(&mut buf))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// An action is a wire op. Every branch sends the op, then reads
    /// the new state back.
    fn dispatch(&mut self, action: Action) -> io::Result<()> {
        let keep_popup = matches!(action, Action::Help);
        let result = match action {
            Action::Detach => {
                self.detached = true;
                Ok(())
            }
            Action::Help => Ok(()),
            Action::NewSession => self.new_session(),
            Action::SwitchSession(n) => self.switch_session(n),
            Action::NewWindow => {
                self.add_window()?;
                self.refresh()
            }
            Action::SplitVertical | Action::SplitHorizontal => {
                let window = self.focused_window();
                if let Some(window) = window {
                    self.split(&window)?;
                }
                self.refresh()
            }
        };
        self.which_key.set_scope(Scope::Global);
        self.which_key.dismiss();
        if keep_popup {
            self.which_key.toggle();
        }
        result
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

    /// Draw one frame: fill the ultimate background, the session list
    /// column, the panes' grids, the status line, and the prefix
    /// popup. The ultimate background (`bg.panel`) shows behind and
    /// between the tiles; each tile's ground is `bg.base`.
    pub fn draw(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        frame.render_widget(Block::default().bg(self.c("bg.panel")), area);

        let wide = area.width >= WIDE_MIN;
        if wide {
            let chunks = Layout::horizontal([
                Constraint::Length(SIDEBAR_COLS),
                Constraint::Length(GAP),
                Constraint::Fill(1),
            ])
            .split(area);
            self.draw_sidebar(frame, chunks[0]);
            self.draw_content(frame, chunks[2]);
        } else {
            self.draw_content(frame, area);
        }
        self.draw_status(frame, area);

        if self.which_key.active || !self.which_key.current_sequence.is_empty() {
            let popup = WhichKey::new().border_style(Style::default().fg(self.c("border.focused")));
            popup.render(frame.buffer_mut(), &self.which_key);
        }
    }

    /// The session list: the attached session wears the accent border;
    /// the rest are muted. The column's background is the frame's
    /// ultimate background.
    fn draw_sidebar(&self, frame: &mut ratatui::Frame, area: Rect) {
        let mut y = area.y;
        let border = self.c("accent.primary");
        let selected = self.c("accent.primary");
        let muted = self.c("text.muted");
        let text = self.c("text.primary");
        for session in &self.sessions {
            if y >= area.bottom() {
                break;
            }
            if self.attached.as_deref() == Some(session.as_str()) {
                frame.buffer_mut().set_stringn(area.x, y, "┃", 1, Style::default().fg(border));
                frame.buffer_mut().set_stringn(area.x + 2, y, session, (area.width.saturating_sub(3)) as usize, Style::default().fg(selected));
            } else {
                frame.buffer_mut().set_stringn(area.x + 2, y, session, (area.width.saturating_sub(3)) as usize, Style::default().fg(muted));
            }
            y += 1;
        }
        if y < area.bottom() {
            frame.buffer_mut().set_stringn(
                area.x + 2,
                y,
                "ctrl-b n — new session",
                (area.width.saturating_sub(3)) as usize,
                Style::default().fg(text).add_modifier(Modifier::DIM),
            );
        }
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
        for window in &view.windows {
            for pane in &window.panes {
                let rect = Rect {
                    x: inner.x + pane.x,
                    y: inner.y + pane.y,
                    width: pane.cols.min(inner.width.saturating_sub(pane.x)),
                    height: pane.rows.min(inner.height.saturating_sub(pane.y)),
                };
                self.draw_pane(frame, &pane.pane, rect, pane.pane == view.focused);
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
    /// the daemon kept. The focused pane's cursor shows.
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
        if focused && grid.alive {
            let col = grid.cursor_col;
            let row = grid.cursor_row;
            if row < rect.height && col < rect.width {
                frame.set_cursor_position((rect.x + col, rect.y + row));
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
        let hints = "ctrl-b prefix · esc detach";
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