//! Parallel app: window → panel. A panel occupies a slot. A panel may
//! hold more slots as rows (a list of any panels), inject a terminal
//! service, or show a line of text. Serve owns services. prefix+q
//! unmounts the window only.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Terminal;

use super::hits;
use super::keys::{Action, Keymap};
use super::term;
use super::theme::{self, Face};
use super::Launch;
use crate::config::Config;
use crate::frame;
use crate::serve::{self, Client, PtyScreen, Service, Spawn};

/// Always put the login shell back.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut out = io::stdout();
        let _ = execute!(
            out,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}

/// One terminal on serve. Detach drops this client, not the shell.
struct Shell {
    client: Client,
    name: String,
}

impl Shell {
    fn attach(launch: &Launch, cols: u16, rows: u16) -> io::Result<Self> {
        let sock = serve::default_sock();
        let mut client = serve::connect_or_spawn(&Spawn {
            root: launch
                .root
                .clone()
                .unwrap_or_else(frame::default_root),
            hammer: launch.hammer.clone(),
            config: launch.config.clone(),
            sock,
        })?;
        let name = "term".to_string();
        client.pty_open(&name, cols.max(2), rows.max(2))?;
        Ok(Self { client, name })
    }

    fn snap(&mut self) -> Option<PtyScreen> {
        self.client.pty_snap(&self.name).ok()
    }

    fn write(&mut self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        let _ = self.client.pty_write(&self.name, &text);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        let _ = self.client.pty_resize(&self.name, cols.max(2), rows.max(2));
    }

    fn services(&mut self) -> Vec<Service> {
        self.client
            .inspect()
            .map(|r| r.services)
            .unwrap_or_default()
    }
}

/// A panel occupies one slot. Rows are more slots, stacked. Any panel
/// may sit in a row — text today, a terminal, or another list.
#[derive(Debug, Clone)]
enum Panel {
    Text(String),
    Terminal,
    Rows {
        children: Vec<Panel>,
        offset: u16,
        stick: bool,
    },
}

impl Panel {
    fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    fn terminal() -> Self {
        Self::Terminal
    }

    fn rows(children: Vec<Panel>) -> Self {
        Self::Rows {
            children,
            offset: 0,
            stick: true,
        }
    }

    /// Fixed height in rows. `None` means this panel takes leftover space.
    fn fixed_h(&self) -> Option<u16> {
        match self {
            Self::Text(_) => Some(1),
            Self::Terminal => None,
            Self::Rows { children, .. } => {
                Some(children.iter().map(|c| c.fixed_h().unwrap_or(1)).sum())
            }
        }
    }

    fn bump(&mut self, delta: i32, view_h: u16) {
        let Self::Rows {
            children,
            offset,
            stick,
        } = self
        else {
            return;
        };
        let n = children.len();
        let max = n.saturating_sub(view_h as usize) as u16;
        if *stick {
            *offset = max;
            *stick = false;
        }
        let next = if delta < 0 {
            offset.saturating_sub(delta.unsigned_abs() as u16)
        } else {
            offset.saturating_add(delta as u16)
        };
        if next >= max {
            *offset = max;
            *stick = true;
        } else {
            *offset = next;
        }
    }
}

fn services_panel(services: &[Service]) -> Panel {
    if services.is_empty() {
        return Panel::rows(vec![Panel::text("(no services)")]);
    }
    Panel::rows(
        services
            .iter()
            .map(|s| Panel::text(format!(" {}  {}  {}", s.name, s.kind, s.state)))
            .collect(),
    )
}

/// Root: a row-list. First child is the service list (also rows).
/// Last child is the terminal and takes leftover height.
fn root_panel(services: &[Service], list_scroll: &Panel) -> Panel {
    let mut list = services_panel(services);
    if let (
        Panel::Rows {
            offset, stick, ..
        },
        Panel::Rows {
            offset: o,
            stick: s,
            ..
        },
    ) = (&mut list, list_scroll)
    {
        *offset = *o;
        *stick = *s;
    }
    Panel::rows(vec![list, Panel::terminal()])
}

/// Stack children as rows. Fixed-height panels keep that height.
/// One flexible panel (the terminal) gets the rest. If fixed children
/// would starve it, they are clipped and that child list scrolls.
fn place_rows(area: Rect, children: &[Panel]) -> Vec<Rect> {
    if children.is_empty() || area.height == 0 {
        return vec![];
    }
    let flex = children.iter().filter(|c| c.fixed_h().is_none()).count() as u16;
    let want: u16 = children.iter().filter_map(Panel::fixed_h).sum();
    let reserve = if flex > 0 { 2 } else { 0 };
    let mut avail_fixed = area.height.saturating_sub(reserve);
    let mut y = area.y;
    let mut out = Vec::with_capacity(children.len());
    let leftover_flex = area.height.saturating_sub(want.min(avail_fixed));
    let flex_h = if flex == 0 {
        0
    } else {
        leftover_flex / flex.max(1)
    };
    for child in children {
        if y >= area.y.saturating_add(area.height) {
            out.push(Rect::new(area.x, y, area.width, 0));
            continue;
        }
        let h = match child.fixed_h() {
            Some(need) => {
                let take = need.min(avail_fixed);
                avail_fixed = avail_fixed.saturating_sub(take);
                take
            }
            None => flex_h
                .max(1)
                .min(area.y.saturating_add(area.height).saturating_sub(y)),
        };
        let h = h.min(area.y.saturating_add(area.height).saturating_sub(y));
        out.push(Rect::new(area.x, y, area.width, h));
        y = y.saturating_add(h);
    }
    out
}

pub fn run(launch: &Launch) -> io::Result<()> {
    let (_path, cfg) = match launch.config.as_deref() {
        Some(p) => Config::load_from(p),
        None => Config::load(),
    }
    .map_err(|err| io::Error::other(err.to_string()))?;
    theme::install(theme::Theme::from_config(&cfg.theme));
    let keys = Keymap::from_config(&cfg.keys);

    enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let size = terminal.size()?;
    let mut shell = Shell::attach(launch, size.width.max(2), size.height.saturating_sub(1).max(2))?;
    event_loop(&mut terminal, &keys, &mut shell)
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    keys: &Keymap,
    shell: &mut Shell,
) -> io::Result<()> {
    let mut prefix_armed = false;
    let mut last_term = (0, 0);
    let mut list_scroll = Panel::rows(vec![]);
    let mut services: Vec<Service> = Vec::new();
    let mut last_inspect = Instant::now() - Duration::from_secs(1);
    let mut pointer: Option<(u16, u16)> = None;
    loop {
        let size = terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        if last_inspect.elapsed() >= Duration::from_millis(250) {
            services = shell.services();
            last_inspect = Instant::now();
        }
        let root = root_panel(&services, &list_scroll);
        let placed = place_root(area, &root);
        if let Some(term_area) = placed.terminal {
            let sz = (term_area.width, term_area.height);
            if sz != last_term {
                shell.resize(term_area.width, term_area.height);
                last_term = sz;
            }
        }
        let screen = shell.snap();
        terminal.draw(|frame| {
            draw_window(frame, prefix_armed, &root, screen.as_ref());
        })?;
        if !event::poll(Duration::from_millis(16))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                match handle_key(keys, &mut prefix_armed, key) {
                    KeyOut::Detach => return Ok(()),
                    KeyOut::ToTerminal(bytes) => shell.write(&bytes),
                    KeyOut::None => {}
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Moved => pointer = Some((mouse.column, mouse.row)),
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let (c, r) = pointer.unwrap_or((mouse.column, mouse.row));
                    if let Some(list_area) = placed.list {
                        if hits::inside(list_area, c, r) {
                            let dir = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                                -3
                            } else {
                                3
                            };
                            list_scroll.bump(dir, list_area.height);
                        }
                    }
                }
                _ => {}
            },
            Event::Resize(cols, rows) => {
                let root = root_panel(&services, &list_scroll);
                let placed = place_root(Rect::new(0, 0, cols, rows), &root);
                if let Some(term_area) = placed.terminal {
                    shell.resize(term_area.width, term_area.height);
                    last_term = (term_area.width, term_area.height);
                }
            }
            Event::Paste(text) => shell.write(text.as_bytes()),
            _ => {}
        }
    }
}

struct Placed {
    list: Option<Rect>,
    terminal: Option<Rect>,
}

fn place_root(area: Rect, root: &Panel) -> Placed {
    let mut out = Placed {
        list: None,
        terminal: None,
    };
    let Panel::Rows { children, .. } = root else {
        return out;
    };
    let rects = place_rows(area, children);
    out.list = rects.first().copied();
    out.terminal = children.iter().zip(rects.iter()).find_map(|(c, r)| {
        matches!(c, Panel::Terminal).then_some(*r)
    });
    out
}

fn draw_window(
    frame: &mut ratatui::Frame,
    prefix_armed: bool,
    root: &Panel,
    screen: Option<&PtyScreen>,
) {
    let th = theme::t();
    let window = frame.area();
    frame.render_widget(Block::default().style(th.style(Face::Canvas)), window);
    paint_panel(frame, window, root, screen);
    if prefix_armed && window.height > 0 {
        let row = Rect::new(window.x, window.y + window.height - 1, window.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " prefix",
                th.style(Face::StatusInk),
            ))),
            row,
        );
    }
}

fn paint_panel(
    frame: &mut ratatui::Frame,
    area: Rect,
    panel: &Panel,
    screen: Option<&PtyScreen>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    match panel {
        Panel::Text(text) => {
            let th = theme::t();
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    text.clone(),
                    th.style(Face::RailRow),
                ))),
                area,
            );
        }
        Panel::Terminal => paint_terminal(frame, area, screen),
        Panel::Rows {
            children,
            offset,
            stick,
        } => {
            let flex = children.iter().any(|c| c.fixed_h().is_none());
            let slice: &[Panel] = if flex {
                children
            } else {
                let view = area.height as usize;
                let max = children.len().saturating_sub(view);
                let start = if *stick {
                    max
                } else {
                    (*offset as usize).min(max)
                };
                &children[start..]
            };
            let rects = place_rows(area, slice);
            for (child, rect) in slice.iter().zip(rects.iter()) {
                paint_panel(frame, *rect, child, screen);
            }
        }
    }
}

fn paint_terminal(frame: &mut ratatui::Frame, panel: Rect, screen: Option<&PtyScreen>) {
    if panel.width == 0 || panel.height == 0 {
        return;
    }
    let lines = term::screen_lines(screen);
    let cursor_row = screen.map(|s| s.cursor_row as usize).unwrap_or(0);
    let scroll = term::cursor_scroll(lines.len(), panel.height as usize, cursor_row);
    let end = (scroll + panel.height as usize).min(lines.len());
    let visible = if scroll == 0 && end == lines.len() {
        lines
    } else {
        lines[scroll..end].to_vec()
    };
    frame.render_widget(Paragraph::new(visible), panel);
    if let Some(s) = screen {
        if s.alive {
            let local_row = s.cursor_row.saturating_sub(scroll as u16);
            let x = panel.x.saturating_add(s.cursor_col);
            let y = panel.y.saturating_add(local_row);
            if x < panel.x.saturating_add(panel.width) && y < panel.y.saturating_add(panel.height) {
                frame.set_cursor_position((x, y));
            }
        }
    }
}

#[derive(Debug)]
enum KeyOut {
    Detach,
    ToTerminal(Vec<u8>),
    None,
}

fn handle_key(keys: &Keymap, prefix_armed: &mut bool, key: KeyEvent) -> KeyOut {
    if *prefix_armed {
        *prefix_armed = false;
        if keys.is_prefix(key) {
            return term::key_bytes(key)
                .map(KeyOut::ToTerminal)
                .unwrap_or(KeyOut::None);
        }
        if matches!(keys.resolve(key, true, |_| true), Some(Action::Detach)) {
            return KeyOut::Detach;
        }
        return KeyOut::None;
    }
    if keys.is_prefix(key) {
        *prefix_armed = true;
        return KeyOut::None;
    }
    match term::key_bytes(key) {
        Some(bytes) if bytes == [0x03] => KeyOut::None,
        Some(bytes) => KeyOut::ToTerminal(bytes),
        None => KeyOut::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_one_row_terminal_is_flexible() {
        assert_eq!(Panel::text("x").fixed_h(), Some(1));
        assert_eq!(Panel::terminal().fixed_h(), None);
        let list = Panel::rows(vec![Panel::text("a"), Panel::text("b")]);
        assert_eq!(list.fixed_h(), Some(2));
    }

    #[test]
    fn rows_stack_fixed_then_leftover_goes_to_terminal() {
        let root = Panel::rows(vec![
            Panel::rows(vec![Panel::text("a"), Panel::text("b")]),
            Panel::terminal(),
        ]);
        let kids = match &root {
            Panel::Rows { children, .. } => children,
            _ => panic!(),
        };
        let rects = place_rows(Rect::new(0, 0, 80, 24), kids);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], Rect::new(0, 0, 80, 2));
        assert_eq!(rects[1].y, 2);
        assert_eq!(rects[1].height, 22);
        assert_eq!(rects[0].x, rects[1].x);
    }

    #[test]
    fn list_scroll_sticks_to_the_bottom() {
        let mut list = Panel::rows((0..20).map(|i| Panel::text(i.to_string())).collect());
        list.bump(1, 5);
        match &list {
            Panel::Rows { stick, .. } => assert!(*stick),
            _ => panic!(),
        }
        list.bump(-3, 5);
        match &list {
            Panel::Rows { stick, offset, .. } => {
                assert!(!*stick);
                assert!(*offset < 15);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn prefix_q_detaches() {
        let keys = Keymap::defaults();
        let mut armed = false;
        let prefix = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL);
        let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(matches!(handle_key(&keys, &mut armed, prefix), KeyOut::None));
        assert!(armed);
        assert!(matches!(
            handle_key(&keys, &mut armed, q),
            KeyOut::Detach
        ));
    }

    #[test]
    fn ctrl_c_is_not_sent_to_the_terminal() {
        let keys = Keymap::defaults();
        let mut armed = false;
        let c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(handle_key(&keys, &mut armed, c), KeyOut::None));
    }

    #[test]
    fn typing_goes_to_the_terminal() {
        let keys = Keymap::defaults();
        let mut armed = false;
        match handle_key(
            &keys,
            &mut armed,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        ) {
            KeyOut::ToTerminal(bytes) => assert_eq!(bytes, b"x"),
            other => panic!("{other:?}"),
        }
    }
}
