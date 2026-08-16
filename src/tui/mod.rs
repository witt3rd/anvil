//! smith TUI: transcript blocks, ask worker, `@` file picker, casing rail.

mod activity;
mod clip;
mod hits;
mod keys;
mod paste;
mod picker;
mod plot;
mod rail;
mod scroll;
mod select;
mod status;
mod term;
mod theme;
mod thumb;

use hits::{row_rect, split_edge_rect, HitKind, Hits, NavDir};
use theme::Face;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use rail::{Focus, Naming, Rail, RailKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainView {
    Smith,
    Trajectory,
}

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use ratatui::Terminal;

use crate::ask::{self, AskSink, HttpCompleter};
use crate::config::{Config, Provider};
use crate::frame::{self, Event as LogEvent, EventBody, FrameRoot, SplitDir, Tile};
use crate::serve::{self, Client, EditBuf, EditOp, PtyScreen, Spawn};
use crate::{default_hammer, Anvil, StrikeReply};

pub use picker::{at_span, insert_path, list_files, rank, FileHit};

#[derive(Debug, Clone)]
pub enum Card {
    User {
        text: String,
    },
    Thinking {
        text: String,
        folded: bool,
        label: Option<String>,
        phase: activity::StepKind,
    },
    Step {
        n: u32,
        timing: crate::prof::Timing,
    },
    Strike {
        code: String,
        stdout: String,
        stderr: String,
        error: Option<String>,
        ok: bool,
        folded: bool,
    },
    Answer {
        text: String,
    },
    Status {
        text: String,
    },
}

#[derive(Debug, Clone)]
enum Job {
    Ask { session: String, prompt: String },
    Strike { session: String, code: String },
    Expose { session: String },
    Mount { kind: String, slot: Option<String> },
    Unmount { id: Option<String> },
}

#[derive(Debug, Clone)]
enum Ev {
    Status(String),
    Delta {
        kind: String,
        text: String,
    },
    Draft(String),
    Reason(String),
    Step {
        n: u32,
        timing: crate::prof::Timing,
    },
    Strike {
        code: String,
        stdout: String,
        stderr: String,
        error: Option<String>,
        ok: bool,
    },
    Answer(String),
    Failed(String),
    Mounted {
        id: String,
        slot: String,
    },
    Unmounted {
        id: String,
    },
}

struct ChanSink {
    tx: Sender<Ev>,
}

impl AskSink for ChanSink {
    fn on_status(&mut self, status: &str) {
        let _ = self.tx.send(Ev::Status(status.into()));
    }
    fn on_delta(&mut self, kind: &str, text: &str) {
        let _ = self.tx.send(Ev::Delta {
            kind: kind.into(),
            text: text.into(),
        });
    }
    fn on_draft(&mut self, text: &str) {
        let _ = self.tx.send(Ev::Draft(text.into()));
    }
    fn on_reason(&mut self, text: &str) {
        let _ = self.tx.send(Ev::Reason(text.into()));
    }
    fn on_step(&mut self, n: u32, timing: &crate::prof::Timing) {
        let _ = self.tx.send(Ev::Step {
            n,
            timing: timing.clone(),
        });
    }
    fn on_strike(&mut self, code: &str, reply: &StrikeReply) {
        let _ = self.tx.send(Ev::Strike {
            code: code.into(),
            stdout: reply.stdout.clone(),
            stderr: reply.stderr.clone(),
            error: reply.error.clone(),
            ok: reply.ok,
        });
    }
}

pub struct Launch {
    /// Raw store escape hatch. When set, no rail — one anonymous session.
    pub store: Option<PathBuf>,
    pub root: Option<PathBuf>,
    pub session: Option<String>,
    pub workspace: Option<String>,
    pub catalog: Option<String>,
    pub hammer: PathBuf,
    pub config: Option<PathBuf>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub cwd: PathBuf,
}

struct App {
    cards: Vec<Card>,
    input: String,
    cursor: usize,
    status: String,
    busy: bool,
    activity: Option<activity::Activity>,
    verbosity: activity::Verbosity,
    pointer: Option<(u16, u16)>,
    hover: Option<String>,
    scroll_under: Option<String>,
    keys: keys::Keymap,
    prefix_armed: bool,
    help: bool,
    help_view: HelpView,
    settings: bool,
    zoom: bool,
    resize_mode: bool,
    copy_mode: bool,
    config_path: PathBuf,
    views: HashMap<String, PaneView>,
    hits: Hits,
    tick: u8,
    files: Vec<String>,
    picker: Option<PickerState>,
    jobs: Sender<Job>,
    events: Receiver<Ev>,
    provider_name: String,
    model: String,
    frame: Option<FrameRoot>,
    rail: Option<Rail>,
    focus: Focus,
    sock: Option<PathBuf>,
    slot_status: Option<String>,
    last_mount: Option<String>,
    view: MainView,
    log_events: Vec<LogEvent>,
    other_cards: HashMap<String, Vec<Card>>,
    other_logs: HashMap<String, Vec<LogEvent>>,
    pty_screen: Option<PtyScreen>,
    other_ptys: HashMap<String, PtyScreen>,
    edit_buf: Option<EditBuf>,
    other_edits: HashMap<String, EditBuf>,
    pty_size: HashMap<String, (u16, u16)>,
    pty_want: HashMap<String, (u16, u16)>,
    copy_on_select: bool,
    status_auto_hide: bool,
    status_widgets: Vec<String>,
    context_window: Option<u32>,
    cwd: PathBuf,
    git_cache: status::GitCache,
    selection: Option<select::Selection>,
    painted: select::Painted,
    last_click: Option<(Instant, u16, u16)>,
    edge_drag: Option<EdgeDrag>,
    toast: Option<Toast>,
    pastes: Vec<paste::Paste>,
    compose_area: Option<Rect>,
    last_esc: Option<Instant>,
    activity_of: Option<String>,
    prof: crate::prof::Snap,
    kitty_blit: Option<thumb::KittyBlit>,
    kitty_cache: Option<(usize, u16, u16, Vec<u8>)>,
    kitty_shown: bool,
}

#[derive(Debug, Clone)]
struct EdgeDrag {
    path: Option<Vec<usize>>,
    gap: usize,
    dir: SplitDir,
    origin: u16,
    px_a: u16,
    px_b: u16,
}

#[derive(Debug, Clone)]
struct Toast {
    message: String,
    until: Instant,
}

#[derive(Debug, Clone, Default)]
struct HelpView {
    query: String,
    scroll: u16,
    max: u16,
    search: bool,
}

struct PickerState {
    query: String,
    hits: Vec<FileHit>,
    selected: usize,
    kind: PickerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    File,
    Catalog,
    Goto,
}

#[derive(Debug, Clone, Copy)]
struct PaneView {
    scroll: u16,
    max: u16,
    stick: bool,
}

impl Default for PaneView {
    fn default() -> Self {
        Self {
            scroll: 0,
            max: 0,
            stick: true,
        }
    }
}

/// Unstick from the *visible* bottom (`max`), not from offset 0.
/// Reaching `max` sticks again so follow-mode and manual scroll agree.
fn apply_scroll(view: &mut PaneView, delta: i32) {
    if view.stick {
        view.scroll = view.max;
        view.stick = false;
    }
    let next = if delta < 0 {
        view.scroll.saturating_sub(delta.unsigned_abs() as u16)
    } else {
        view.scroll.saturating_add(delta as u16)
    };
    if next >= view.max {
        view.scroll = view.max;
        view.stick = true;
    } else {
        view.scroll = next;
        view.stick = false;
    }
}

impl App {
    fn expose_live(&self) {
        let _ = self.jobs.send(Job::Expose {
            session: self.session_id(),
        });
    }

    fn session_id(&self) -> String {
        self.rail
            .as_ref()
            .map(|r| r.session.clone())
            .unwrap_or_else(|| "default".into())
    }

    fn scroll_key(&self) -> String {
        match self.view {
            MainView::Trajectory => format!("{}::log", self.session_id()),
            MainView::Smith => self.session_id(),
        }
    }

    fn pane_view(&self, id: &str) -> PaneView {
        self.views.get(id).copied().unwrap_or_default()
    }

    fn stick_focused(&mut self) {
        let id = self.scroll_key();
        self.views.entry(id).or_default().stick = true;
    }

    fn bump_scroll(&mut self, id: &str, delta: i32) {
        let view = self.views.entry(id.to_string()).or_default();
        let from = view.scroll;
        let from_stick = view.stick;
        apply_scroll(view, delta);
        crate::prof::record(crate::prof::Sample {
            name: "tui.scroll".into(),
            group: "tui".into(),
            t0_ns: crate::prof::now_ns(),
            dur_ns: 0,
            tokens: None,
            extra: Some(format!(
                "{id} d={delta} {from}->{} max={} stick {from_stick}->{}",
                view.scroll, view.max, view.stick
            )),
        });
    }

    fn remember_scroll(&mut self, id: &str, max: u16) {
        let view = self.views.entry(id.to_string()).or_default();
        view.max = max;
        if view.stick || view.scroll > max {
            view.scroll = max;
            view.stick = true;
        }
    }

    fn scroll_window(&mut self, id: &str, area: Rect, nlines: usize) -> scroll::Window {
        let max = nlines.saturating_sub(area.height as usize) as u16;
        self.remember_scroll(id, max);
        let view = self.pane_view(id);
        scroll::Window::open(area, nlines, view.scroll, view.stick)
    }

    fn click_workspace(&mut self, name: &str) {
        if self.busy {
            self.status = "busy — wait to switch".into();
            return;
        }
        if let (Some(root), Some(rail)) = (&self.frame, self.rail.as_mut()) {
            match rail.select_workspace(root, name) {
                Ok(_) => {
                    self.load_session_cards();
                    self.expose_live();
                    self.status = format!("sash {name}");
                }
                Err(err) => self.status = err.to_string(),
            }
        }
    }

    fn click_member(&mut self, id: &str, focus: Focus) {
        if self.busy && id != self.session_id() {
            self.status = "busy — wait to switch".into();
            return;
        }
        self.focus = focus;
        if let (Some(root), Some(rail)) = (&self.frame, self.rail.as_mut()) {
            match rail.select_member(root, id) {
                Ok(true) => {
                    self.load_session_cards();
                    self.expose_live();
                    self.status = format!("session {id}");
                }
                Ok(false) => {}
                Err(err) => self.status = err.to_string(),
            }
        }
    }

    /// Hover focus: switch the front member without flushing the layout.
    fn hover_member(&mut self, id: &str, focus: Focus) -> bool {
        if id.ends_with("::log") {
            let changed = self.view != MainView::Trajectory || self.focus != Focus::Compose;
            self.view = MainView::Trajectory;
            self.focus = Focus::Compose;
            return changed;
        }
        if self.busy && id != self.session_id() {
            return false;
        }
        self.focus = focus;
        self.view = MainView::Smith;
        if let Some(rail) = self.rail.as_mut() {
            if rail.peek_member(id) {
                self.load_session_cards();
                self.expose_live();
                return true;
            }
        }
        false
    }

    fn focused_is_pty(&self) -> bool {
        self.rail.as_ref().is_some_and(|r| r.focused_is_pty())
    }

    fn member_is_pty(&self, id: &str) -> bool {
        self.rail
            .as_ref()
            .is_some_and(|r| r.ptys.iter().any(|p| p == id))
    }

    fn member_is_log(&self, id: &str) -> bool {
        self.rail.as_ref().is_some_and(|r| r.member_is_log(id))
    }

    fn member_is_plot(&self, id: &str) -> bool {
        self.rail.as_ref().is_some_and(|r| r.member_is_plot(id))
    }

    fn focused_is_edit(&self) -> bool {
        self.rail.as_ref().is_some_and(|r| r.focused_is_edit())
    }

    fn member_is_edit(&self, id: &str) -> bool {
        self.rail
            .as_ref()
            .is_some_and(|r| r.edits.iter().any(|e| e == id))
    }

    fn member_is_clock(&self, id: &str) -> bool {
        self.rail.as_ref().is_some_and(|r| r.member_is_clock(id))
    }

    fn member_is_session(&self, id: &str) -> bool {
        !self.member_is_pty(id)
            && !self.member_is_edit(id)
            && !self.member_is_plot(id)
            && !self.member_is_log(id)
            && !self.member_is_clock(id)
    }

    fn push_card(&mut self, card: Card) {
        // Serve appends the event log. The casing only projects.
        self.cards.push(card);
    }

    fn push_phase_card(&mut self, step: activity::Step) {
        if step.kind == activity::StepKind::Prefill && step.body.is_empty() && step.tokens == 0 {
            return;
        }
        if step.kind == activity::StepKind::Tool {
            return;
        }
        let folded = self.verbosity != activity::Verbosity::Full;
        self.push_card(Card::Thinking {
            text: step.body,
            folded,
            label: Some(step.title),
            phase: step.kind,
        });
    }

    fn load_session_cards(&mut self) {
        self.cards.clear();
        self.log_events.clear();
        if self.member_is_log(&self.session_id()) {
            if let (Some(root), Some(rail)) = (&self.frame, &self.rail) {
                if let Some(of) = rail.log_of(&self.session_id()) {
                    if let Ok(events) = root.load_events(of) {
                        self.log_events = events;
                    }
                }
            }
        } else if self.member_is_plot(&self.session_id()) {
            if let (Some(root), Some(rail)) = (&self.frame, &self.rail) {
                if let Some(of) = rail.plot_of(&self.session_id()) {
                    if let Ok(events) = root.load_events(of) {
                        self.log_events = events;
                    }
                }
            }
        } else if !self.focused_is_pty() && !self.focused_is_edit() {
            self.reload_log();
            let start = self.log_events.len().saturating_sub(200);
            self.cards = self.log_events[start..]
                .iter()
                .filter_map(card_from_event)
                .collect();
        }
        self.load_others();
        self.stick_focused();
        self.refresh_live();
    }

    fn other_ids(&self) -> Vec<String> {
        self.rail
            .as_ref()
            .map(|r| r.other_members())
            .unwrap_or_default()
    }

    fn load_others(&mut self) {
        self.other_cards.clear();
        self.other_ptys.clear();
        self.other_logs.clear();
        let Some(root) = &self.frame else {
            return;
        };
        for id in self.other_ids() {
            if self.member_is_pty(&id) || self.member_is_edit(&id) {
                continue;
            }
            if let Some(of) = self
                .rail
                .as_ref()
                .and_then(|r| r.log_of(&id).map(str::to_string))
            {
                if let Ok(events) = root.load_events(&of) {
                    self.other_logs.insert(id, events);
                }
                continue;
            }
            if let Ok(events) = root.load_events(&id) {
                let start = events.len().saturating_sub(200);
                self.other_cards.insert(
                    id,
                    events[start..].iter().filter_map(card_from_event).collect(),
                );
            }
        }
    }

    fn with_pty_client<T>(
        &self,
        f: impl FnOnce(&mut Client) -> io::Result<T>,
    ) -> Result<T, String> {
        let sock = self
            .sock
            .as_ref()
            .ok_or_else(|| "pty needs serve".to_string())?;
        let mut client = Client::connect(sock).map_err(|err| err.to_string())?;
        let _ = client.set_timeout(Duration::from_millis(400));
        f(&mut client).map_err(|err| err.to_string())
    }

    fn needs_live_snap(&self) -> bool {
        self.focused_is_pty()
            || self.focused_is_edit()
            || self
                .other_ids()
                .iter()
                .any(|id| self.member_is_pty(id) || self.member_is_edit(id))
    }

    fn refresh_live(&mut self) {
        let _g = crate::prof::span("tui.snap", "tui");
        let Some(sock) = self.sock.clone() else {
            return;
        };
        let Ok(mut client) = Client::connect(&sock) else {
            return;
        };
        let _ = client.set_timeout(Duration::from_millis(150));
        self.refresh_ptys_with(&mut client);
        self.refresh_edits_with(&mut client);
    }

    fn refresh_ptys_with(&mut self, client: &mut Client) {
        if self.focused_is_pty() {
            let name = self.session_id();
            match client.pty_snap(&name) {
                Ok(screen) => self.pty_screen = Some(screen),
                Err(err) => self.status = err.to_string(),
            }
        } else {
            self.pty_screen = None;
        }
        let ids: Vec<String> = self
            .other_ids()
            .into_iter()
            .filter(|id| self.member_is_pty(id))
            .collect();
        let mut ptys = HashMap::new();
        for id in ids {
            match client.pty_snap(&id) {
                Ok(screen) => {
                    ptys.insert(id, screen);
                }
                Err(err) => self.status = err.to_string(),
            }
        }
        self.other_ptys = ptys;
    }

    fn refresh_edits_with(&mut self, client: &mut Client) {
        if self.focused_is_edit() {
            let name = self.session_id();
            match client.edit_snap(&name) {
                Ok(buf) => self.edit_buf = Some(buf),
                Err(err) => self.status = err.to_string(),
            }
        } else {
            self.edit_buf = None;
        }
        let ids: Vec<String> = self
            .other_ids()
            .into_iter()
            .filter(|id| self.member_is_edit(id))
            .collect();
        let mut edits = HashMap::new();
        for id in ids {
            match client.edit_snap(&id) {
                Ok(buf) => {
                    edits.insert(id, buf);
                }
                Err(err) => self.status = err.to_string(),
            }
        }
        self.other_edits = edits;
    }

    fn pull_status(&mut self) -> bool {
        let Some(sock) = self.sock.clone() else {
            return false;
        };
        let Ok(mut client) = Client::connect(&sock) else {
            return false;
        };
        let _ = client.set_timeout(Duration::from_millis(150));
        let Ok(report) = client.inspect() else {
            return false;
        };
        let next = report
            .slots
            .iter()
            .find(|s| s.name == "casing.status")
            .and_then(|s| s.text.clone());
        let prof_changed = report.prof.samples.len() != self.prof.samples.len()
            || report.prof.samples.last().map(|s| s.t0_ns)
                != self.prof.samples.last().map(|s| s.t0_ns)
            || report.prof.last_model != self.prof.last_model;
        self.prof = report.prof;
        if self.slot_status == next && !prof_changed {
            return false;
        }
        self.slot_status = next;
        true
    }

    fn refresh_chrome(&mut self) {
        let mut cwds = vec![self.cwd.clone()];
        if let (Some(root), Some(rail)) = (&self.frame, &self.rail) {
            for id in &rail.members {
                if self.member_is_pty(id)
                    || self.member_is_edit(id)
                    || self.member_is_plot(id)
                    || self.member_is_log(id)
                    || self.member_is_clock(id)
                {
                    continue;
                }
                if let Ok(s) = root.session(id) {
                    if let Some(c) = s.meta.cwd {
                        cwds.push(PathBuf::from(c));
                    }
                }
            }
        }
        for cwd in cwds {
            let _ = status::refresh_git(&cwd, &mut self.git_cache);
        }
    }

    fn send_edit(&mut self, op: EditOp, text: &str) {
        let name = self.session_id();
        match self.with_pty_client(|c| c.edit(&name, op, text)) {
            Ok(buf) => self.edit_buf = Some(buf),
            Err(err) => self.status = err,
        }
    }

    fn send_pty(&mut self, data: &[u8]) {
        let name = self.session_id();
        let payload = String::from_utf8_lossy(data).into_owned();
        match self.with_pty_client(|c| c.pty_write(&name, &payload)) {
            Ok(screen) => self.pty_screen = Some(screen),
            Err(err) => self.status = err,
        }
    }

    fn reload_log(&mut self) {
        self.log_events.clear();
        let Some(root) = &self.frame else {
            return;
        };
        if let Ok(events) = root.load_events(&self.session_id()) {
            self.log_events = events;
        }
    }

    fn take_compose(&mut self) -> String {
        let raw = std::mem::take(&mut self.input);
        self.cursor = 0;
        self.picker = None;
        let text = paste::expand(&raw, &self.pastes);
        self.pastes.clear();
        text.trim_end().to_string()
    }

    fn clear_compose(&mut self) {
        let had = !self.input.is_empty() || !self.pastes.is_empty();
        self.input.clear();
        self.cursor = 0;
        self.pastes.clear();
        self.picker = None;
        self.last_esc = None;
        self.status = if had {
            "compose cleared".into()
        } else {
            "idle".into()
        };
    }

    fn submit_ask(&mut self) {
        let text = self.take_compose();
        if text.is_empty() || self.busy {
            return;
        }
        if let Some(slash) = parse_slash(&text) {
            self.dispatch_slash(slash);
            return;
        }
        self.push_card(Card::User { text: text.clone() });
        self.busy = true;
        self.status = "waiting".into();
        self.activity = Some(activity::Activity::start());
        self.activity_of = Some(self.session_id());
        self.stick_focused();
        let _ = self.jobs.send(Job::Ask {
            session: self.session_id(),
            prompt: text,
        });
    }

    fn dispatch_slash(&mut self, slash: Slash) {
        match slash {
            Slash::Mount { kind, slot } => self.mount(&kind, slot.as_deref()),
            Slash::Unmount { id } => self.unmount(id.as_deref()),
        }
    }

    fn mount(&mut self, kind: &str, slot: Option<&str>) {
        if self.sock.is_none() {
            self.push_card(Card::Status {
                text: "mount needs serve (no --store)".into(),
            });
            return;
        }
        self.status = format!("mount {kind}");
        let _ = self.jobs.send(Job::Mount {
            kind: kind.into(),
            slot: slot.map(str::to_string),
        });
    }

    fn unmount(&mut self, id: Option<&str>) {
        if self.sock.is_none() {
            self.push_card(Card::Status {
                text: "unmount needs serve".into(),
            });
            return;
        }
        let id = id.map(str::to_string).or_else(|| self.last_mount.clone());
        self.status = "unmount".into();
        let _ = self.jobs.send(Job::Unmount { id });
    }

    fn submit_strike(&mut self) {
        let text = self.take_compose();
        if text.is_empty() || self.busy {
            return;
        }
        self.push_card(Card::User {
            text: format!("(strike)\n{text}"),
        });
        self.busy = true;
        self.status = "striking".into();
        self.stick_focused();
        let _ = self.jobs.send(Job::Strike {
            session: self.session_id(),
            code: text,
        });
    }

    fn insert(&mut self, ch: char) {
        self.insert_str(&ch.to_string());
    }

    fn insert_str(&mut self, s: &str) {
        self.input.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.refresh_picker();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some(span) = paste::chip_covering(&self.input, self.cursor, &self.pastes) {
            self.input.drain(span.start..span.end);
            self.cursor = span.start;
            if span.index < self.pastes.len() {
                self.pastes.remove(span.index);
            }
            self.refresh_picker();
            return;
        }
        let prev = self.input[..self.cursor]
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        let start = self.cursor - prev;
        self.input.drain(start..self.cursor);
        self.cursor = start;
        self.refresh_picker();
    }

    fn toast(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.status = message.clone();
        self.toast = Some(Toast {
            message,
            until: Instant::now() + Duration::from_millis(1600),
        });
    }

    fn refresh_picker(&mut self) {
        match at_span(&self.input, self.cursor) {
            None => self.picker = None,
            Some((_, query)) => {
                let hits = rank(&self.files, &query, 12);
                let selected = self
                    .picker
                    .as_ref()
                    .map(|p| p.selected.min(hits.len().saturating_sub(1)))
                    .unwrap_or(0);
                self.picker = Some(PickerState {
                    query,
                    hits,
                    selected,
                    kind: PickerKind::File,
                });
            }
        }
    }

    fn accept_picker(&mut self) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        if picker.hits.is_empty() {
            return;
        }
        let path = picker.hits[picker.selected.min(picker.hits.len() - 1)]
            .path
            .clone();
        let kind = picker.kind;
        self.picker = None;
        match kind {
            PickerKind::File => {
                if let Some((next, cur)) = insert_path(&self.input, self.cursor, &path) {
                    self.input = next;
                    self.cursor = cur;
                }
            }
            PickerKind::Catalog => {
                if let (Some(root), Some(rail)) = (&self.frame, self.rail.as_mut()) {
                    match rail.select_catalog(root, &path) {
                        Ok(_) => {
                            self.load_session_cards();
                            self.expose_live();
                            self.status = format!("catalog {path}");
                        }
                        Err(err) => self.status = err.to_string(),
                    }
                }
            }
            PickerKind::Goto => {
                if let Some((ws, member)) = path.split_once('/') {
                    if let (Some(root), Some(rail)) = (&self.frame, self.rail.as_mut()) {
                        let _ = rail.select_workspace(root, ws);
                    }
                    self.click_member(member, Focus::Compose);
                    self.status = format!("goto {path}");
                } else if let (Some(root), Some(rail)) = (&self.frame, self.rail.as_mut()) {
                    match rail.select_workspace(root, &path) {
                        Ok(_) => {
                            self.load_session_cards();
                            self.expose_live();
                            self.status = format!("sash {path}");
                        }
                        Err(err) => self.status = err.to_string(),
                    }
                }
            }
        }
    }

    fn apply(&mut self, ev: Ev) {
        match ev {
            Ev::Status(s) => {
                let closed = self.activity.as_mut().and_then(|act| act.on_status(&s));
                if let Some(step) = closed {
                    self.push_phase_card(step);
                }
                self.status = s;
            }
            Ev::Delta { kind, text } => {
                let closed = self.activity.as_mut().and_then(|act| act.on_delta(&kind, &text));
                if let Some(step) = closed {
                    self.push_phase_card(step);
                }
                self.status = match kind.as_str() {
                    "reason" => "think".into(),
                    _ => "decode".into(),
                };
            }
            Ev::Reason(text) => {
                if let Some(act) = &mut self.activity {
                    if !act.steps.iter().any(|s| s.kind == activity::StepKind::Think) {
                        let _ = act.on_delta("reason", &text);
                    }
                }
            }
            Ev::Step { n, timing } => {
                self.push_card(Card::Step { n, timing });
            }
            Ev::Draft(_text) => {
                let closed = self.activity.as_mut().and_then(|act| act.close_open_take());
                if let Some(step) = closed {
                    self.push_phase_card(step);
                }
            }
            Ev::Strike {
                code,
                stdout,
                stderr,
                error,
                ok,
            } => {
                if let Some(act) = &mut self.activity {
                    let closed = act.on_strike(&code, &stdout, ok, None);
                    if let Some(step) = closed {
                        self.push_phase_card(step);
                    }
                }
                let folded = code.lines().count() > 8;
                self.push_card(Card::Strike {
                    code,
                    stdout,
                    stderr,
                    error,
                    ok,
                    folded,
                });
                self.reload_log();
                self.load_others();
            }
            Ev::Answer(text) => {
                if !text.is_empty() {
                    self.push_card(Card::Answer { text });
                }
                self.busy = false;
                self.status = "idle".into();
                if let Some(act) = &mut self.activity {
                    if let Some(step) = act.finish() {
                        self.push_phase_card(step);
                    }
                }
                self.stick_focused();
                self.reload_log();
            }
            Ev::Failed(text) => {
                self.push_card(Card::Status { text });
                self.busy = false;
                self.status = "error".into();
                if let Some(act) = &mut self.activity {
                    if let Some(step) = act.finish() {
                        self.push_phase_card(step);
                    }
                }
            }
            Ev::Mounted { id, slot } => {
                self.last_mount = Some(id.clone());
                self.status = format!("mounted {id}");
                self.push_card(Card::Status {
                    text: format!("mounted {id} on {slot}"),
                });
            }
            Ev::Unmounted { id } => {
                if self.last_mount.as_deref() == Some(id.as_str()) {
                    self.last_mount = None;
                }
                self.slot_status = None;
                self.status = format!("unmounted {id}");
                self.push_card(Card::Status {
                    text: format!("unmounted {id}"),
                });
            }
        }
    }

    fn toggle_last_fold(&mut self) {
        match self.cards.last_mut() {
            Some(Card::Thinking { folded, .. } | Card::Strike { folded, .. }) => {
                *folded = !*folded;
            }
            _ => {}
        }
    }
}

pub fn run(launch: Launch) -> io::Result<()> {
    let (cfg_path, cfg) = match launch.config.as_deref() {
        Some(p) => Config::load_from(p),
        None => Config::load(),
    }
    .map_err(|err| io::Error::other(err.to_string()))?;
    theme::install(theme::Theme::from_config(&cfg.theme));
    let keymap = keys::Keymap::from_config(&cfg.keys);
    let (provider_name, provider) = cfg
        .provider(launch.provider.as_deref())
        .map_err(|err| io::Error::other(err.to_string()))?;
    let provider_name = provider_name.to_string();
    let provider: Provider = provider.clone();
    let model = cfg
        .model_for(&provider, launch.model.as_deref())
        .ok_or_else(|| io::Error::other("no model: set default_model or pass --model"))?;

    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    let (ev_tx, ev_rx) = mpsc::channel::<Ev>();
    let hammer = launch.hammer.clone();
    let worker_provider = provider.clone();
    let worker_model = model.clone();

    let (frame, rail, opener) = if let Some(store) = launch.store.clone() {
        (None, None, WorkerOpen::Raw(store))
    } else {
        let root = FrameRoot::open(launch.root.clone().unwrap_or_else(frame::default_root))
            .map_err(|err| io::Error::other(err.to_string()))?;
        let rail = Rail::load(
            &root,
            launch.catalog.as_deref(),
            launch.workspace.as_deref(),
            launch.session.as_deref(),
        )
        .map_err(|err| io::Error::other(err.to_string()))?;
        let sock = serve::default_sock();
        serve::connect_or_spawn(&Spawn {
            root: root.root().to_path_buf(),
            hammer: hammer.clone(),
            config: launch.config.clone(),
            sock: sock.clone(),
        })
        .map_err(|err| io::Error::other(format!("anvil serve: {err}")))?;
        if let Ok(mut c) = Client::connect(&sock) {
            let _ = c.warm(&rail.workspace);
        }
        (Some(root), Some(rail), WorkerOpen::Serve { sock })
    };
    let inspect_sock = match &opener {
        WorkerOpen::Serve { sock } => Some(sock.clone()),
        WorkerOpen::Raw(_) => None,
    };

    let worker_provider_name = provider_name.clone();
    thread::Builder::new()
        .name("smith-anvil".into())
        .spawn(move || {
            worker(
                opener,
                hammer,
                worker_provider,
                worker_provider_name,
                worker_model,
                jobs_rx,
                ev_tx,
            )
        })
        .map_err(io::Error::other)?;

    let files = list_files(&launch.cwd, 4000);
    let session_label = rail
        .as_ref()
        .map(|r| r.session.clone())
        .unwrap_or_else(|| "store".into());
    let mut app = App {
        cards: vec![Card::Status {
            text: format!(
                "smith · {session_label} · {provider_name} · {model} · {} · Tab rail  n session  p pty  Alt+L log  /mount clock  Ctrl+S strike  Ctrl+C close  Ctrl+Q close pty",
                cfg_path.display()
            ),
        }],
        input: String::new(),
        cursor: 0,
        status: "idle".into(),
        busy: false,
        activity: None,
        verbosity: activity::Verbosity::Steps,
        pointer: None,
        hover: None,
        scroll_under: None,
        views: HashMap::new(),
        hits: Hits::default(),
        keys: keymap,
        prefix_armed: false,
        help: false,
        help_view: HelpView::default(),
        settings: false,
        zoom: false,
        resize_mode: false,
        copy_mode: false,
        config_path: cfg_path.clone(),
        tick: 0,
        files,
        picker: None,
        jobs: jobs_tx,
        events: ev_rx,
        provider_name: provider_name.clone(),
        model: model.clone(),
        frame,
        rail,
        focus: Focus::Compose,
        sock: inspect_sock,
        slot_status: None,
        last_mount: None,
        view: MainView::Smith,
        log_events: Vec::new(),
        other_cards: HashMap::new(),
        other_logs: HashMap::new(),
        pty_screen: None,
        other_ptys: HashMap::new(),
        edit_buf: None,
        other_edits: HashMap::new(),
        pty_size: HashMap::new(),
        pty_want: HashMap::new(),
        copy_on_select: cfg.ui.copy_on_select,
        status_auto_hide: cfg.ui.status_auto_hide,
        status_widgets: cfg.ui.status_widgets(),
        context_window: cfg.ui.context_window,
        cwd: launch.cwd.clone(),
        git_cache: status::GitCache::new(),
        selection: None,
        painted: select::Painted::default(),
        last_click: None,
        edge_drag: None,
        toast: None,
        pastes: Vec::new(),
        compose_area: None,
        last_esc: None,
        activity_of: None,
        prof: crate::prof::Snap::default(),
        kitty_blit: None,
        kitty_cache: None,
        kitty_shown: false,
    };
    if app.frame.is_some() {
        app.load_session_cards();
        app.expose_live();
        if app.cards.is_empty() {
            app.cards.push(Card::Status {
                text: format!(
                    "smith · {} · {provider_name} · {model} · Tab rail  n session  p pty  Enter expose",
                    app.session_id()
                ),
            });
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    app.refresh_chrome();
    let result = event_loop(&mut terminal, &mut app);
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;
    result
}

enum WorkerOpen {
    Raw(PathBuf),
    Serve { sock: PathBuf },
}

fn worker(
    opener: WorkerOpen,
    hammer: PathBuf,
    provider: Provider,
    provider_name: String,
    model: String,
    jobs: Receiver<Job>,
    ev: Sender<Ev>,
) {
    match opener {
        WorkerOpen::Raw(store) => {
            worker_local(store, hammer, provider, model, jobs, ev);
        }
        WorkerOpen::Serve { sock } => {
            worker_serve(sock, provider_name, model, jobs, ev);
        }
    }
}

fn worker_serve(
    sock: PathBuf,
    provider_name: String,
    model: String,
    jobs: Receiver<Job>,
    ev: Sender<Ev>,
) {
    let mut client = match Client::connect(&sock) {
        Ok(c) => c,
        Err(err) => {
            let _ = ev.send(Ev::Failed(format!("serve: {err}")));
            return;
        }
    };
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Ask { session, prompt } => {
                let mut sink = ChanSink { tx: ev.clone() };
                match client.ask(
                    &session,
                    &prompt,
                    Some(&provider_name),
                    Some(&model),
                    &mut sink,
                ) {
                    Ok(answer) => {
                        let _ = ev.send(Ev::Answer(answer));
                    }
                    Err(err) => {
                        let _ = ev.send(Ev::Failed(err.to_string()));
                    }
                }
            }
            Job::Strike { session, code } => {
                let _ = ev.send(Ev::Status("striking".into()));
                match client.strike(&session, &code) {
                    Ok(reply) => send_strike_ev(&ev, code, reply),
                    Err(err) => {
                        let _ = ev.send(Ev::Failed(err.to_string()));
                    }
                }
            }
            Job::Expose { session } => {
                if let Err(err) = client.expose(&session) {
                    let _ = ev.send(Ev::Failed(err.to_string()));
                }
            }
            Job::Mount { kind, slot } => match client.mount(&kind, slot.as_deref()) {
                Ok((id, seat)) => {
                    let _ = ev.send(Ev::Mounted { id, slot: seat });
                }
                Err(err) => {
                    let _ = ev.send(Ev::Failed(err.to_string()));
                }
            },
            Job::Unmount { id } => {
                let id = match id {
                    Some(id) => id,
                    None => match client.inspect() {
                        Ok(report) => report
                            .slots
                            .iter()
                            .find(|s| s.name == "casing.status")
                            .and_then(|s| s.occupant.clone())
                            .unwrap_or_default(),
                        Err(err) => {
                            let _ = ev.send(Ev::Failed(err.to_string()));
                            continue;
                        }
                    },
                };
                if id.is_empty() {
                    let _ = ev.send(Ev::Failed("nothing mounted on casing.status".into()));
                    continue;
                }
                match client.unmount(&id) {
                    Ok(()) => {
                        let _ = ev.send(Ev::Unmounted { id });
                    }
                    Err(err) => {
                        let _ = ev.send(Ev::Failed(err.to_string()));
                    }
                }
            }
        }
    }
}

fn worker_local(
    store: PathBuf,
    hammer: PathBuf,
    provider: Provider,
    model: String,
    jobs: Receiver<Job>,
    ev: Sender<Ev>,
) {
    let mut anvil = match Anvil::open(&store, &hammer) {
        Ok(a) => a,
        Err(err) => {
            let _ = ev.send(Ev::Failed(err.to_string()));
            return;
        }
    };
    let mut llm = HttpCompleter { provider, model };
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Ask { prompt, .. } => {
                let mut sink = ChanSink { tx: ev.clone() };
                match ask::ask_with(&mut llm, &mut anvil, &prompt, &mut sink) {
                    Ok(result) => {
                        let _ = ev.send(Ev::Answer(result.answer));
                    }
                    Err(err) => {
                        let _ = ev.send(Ev::Failed(err.to_string()));
                    }
                }
            }
            Job::Strike { code, .. } => {
                let _ = ev.send(Ev::Status("striking".into()));
                match anvil.strike(&code) {
                    Ok(reply) => send_strike_ev(&ev, code, reply),
                    Err(err) => {
                        let _ = ev.send(Ev::Failed(err.to_string()));
                    }
                }
            }
            Job::Expose { .. } => {}
            Job::Mount { .. } | Job::Unmount { .. } => {
                let _ = ev.send(Ev::Failed("mount needs serve (no --store)".into()));
            }
        }
    }
}

fn send_strike_ev(ev: &Sender<Ev>, code: String, reply: StrikeReply) {
    let _ = ev.send(Ev::Strike {
        code,
        stdout: reply.stdout.clone(),
        stderr: reply.stderr.clone(),
        error: reply.error.clone(),
        ok: reply.ok,
    });
    let answer = if reply.ok {
        if !reply.stdout.trim().is_empty() {
            reply.stdout.trim().to_string()
        } else if reply.value.is_null() {
            String::new()
        } else {
            reply.value.to_string()
        }
    } else {
        reply
            .error
            .clone()
            .unwrap_or_else(|| "strike failed".into())
    };
    let _ = ev.send(Ev::Answer(answer));
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    let mut last_tick = Instant::now();
    let mut last_inspect = Instant::now();
    let mut last_snap = Instant::now();
    let mut dirty = true;
    loop {
        while let Ok(ev) = app.events.try_recv() {
            app.apply(ev);
            dirty = true;
        }

        let mut quit = false;
        if event::poll(Duration::ZERO)? {
            let _input = crate::prof::span("tui.input", "tui");
            loop {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        if handle_key(app, key) {
                            quit = true;
                            break;
                        }
                        dirty = true;
                    }
                    Event::Mouse(mouse) => {
                        if handle_mouse(app, mouse) {
                            dirty = true;
                        }
                    }
                    Event::Paste(text) => {
                        handle_paste(app, text);
                        dirty = true;
                    }
                    Event::Resize(_, _) => {
                        app.pty_size.clear();
                        dirty = true;
                    }
                    _ => {}
                }
                if quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if quit {
            return Ok(());
        }

        if app.busy && last_tick.elapsed() >= Duration::from_millis(120) {
            app.tick = app.tick.wrapping_add(1);
            last_tick = Instant::now();
            dirty = true;
        }

        if app.needs_live_snap() && last_snap.elapsed() >= Duration::from_millis(80) {
            app.refresh_live();
            last_snap = Instant::now();
            dirty = true;
        }

        if last_inspect.elapsed() >= Duration::from_millis(400) {
            last_inspect = Instant::now();
            app.refresh_chrome();
            let _ = app.pull_status();
            dirty = true;
        }

        if app.toast.as_ref().is_some_and(|t| t.until <= Instant::now()) {
            app.toast = None;
            dirty = true;
        }

        if dirty {
            {
                let _g = crate::prof::span("tui.draw", "tui");
                crate::prof::counter("tui.frame", 1);
                terminal.draw(|frame| draw(frame, app))?;
            }
            let resized = flush_pty_sizes(app);
            if let Some(blit) = &app.kitty_blit {
                if thumb::kitty_place(blit.area, &blit.png) {
                    app.kitty_shown = true;
                }
            } else if app.kitty_shown {
                thumb::kitty_clear();
                app.kitty_shown = false;
            }
            dirty = resized;
        }

        let wait = if app.busy {
            Duration::from_millis(32)
        } else if app.needs_live_snap() {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(80)
        };
        let _ = event::poll(wait);
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn mouse_action(kind: MouseEventKind) -> bool {
    matches!(
        kind,
        MouseEventKind::Down(_)
            | MouseEventKind::Up(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
    )
}

fn note_pty_cell(app: &mut App, id: &str, inner: Rect) {
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    app.pty_want
        .insert(id.to_string(), (inner.width.max(2), inner.height.max(2)));
}

fn flush_pty_sizes(app: &mut App) -> bool {
    let wants: Vec<(String, u16, u16)> = app
        .pty_want
        .iter()
        .filter_map(|(name, &(cols, rows))| match app.pty_size.get(name) {
            Some(&(c, r)) if c == cols && r == rows => None,
            _ => Some((name.clone(), cols, rows)),
        })
        .collect();
    if wants.is_empty() {
        return false;
    }
    let mut changed = false;
    for (name, cols, rows) in wants {
        match app.with_pty_client(|c| c.pty_resize(&name, cols, rows)) {
            Ok(screen) => {
                app.pty_size.insert(name.clone(), (cols, rows));
                if app.focused_is_pty() && app.session_id() == name {
                    app.pty_screen = Some(screen);
                } else {
                    app.other_ptys.insert(name, screen);
                }
                changed = true;
            }
            Err(err) => app.status = err,
        }
    }
    changed
}

fn cycle_sash(app: &mut App, delta: isize) {
    if app.busy {
        app.status = "busy — wait to switch".into();
        return;
    }
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.cycle_sash(root, delta) {
            Ok(_) => {
                app.load_session_cards();
                app.expose_live();
                app.status = format!(
                    "sash {}",
                    app.rail
                        .as_ref()
                        .map(|r| r.workspace.as_str())
                        .unwrap_or("")
                );
            }
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn bump_weight(app: &mut App, delta: i16) {
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        if let Err(err) = rail.bump_weight(root, delta) {
            app.status = err.to_string();
        }
    }
}

fn split_pane(app: &mut App, dir: SplitDir) {
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.split_pane(root, dir) {
            Ok(name) => {
                app.load_session_cards();
                app.expose_live();
                app.focus = Focus::Compose;
                app.status = format!("split {name}");
            }
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn step_member(app: &mut App, delta: isize) {
    let id = app.rail.as_mut().and_then(|r| r.step_member(delta));
    if let Some(id) = id {
        app.hover_member(&id, Focus::Rail);
    } else {
        app.focus = Focus::Rail;
    }
}

fn step_workspace(app: &mut App, delta: isize) {
    app.focus = Focus::Rail;
    if let Some(rail) = app.rail.as_mut() {
        if let Some(name) = rail.step_workspace(delta) {
            rail.peek_row(RailKind::Workspace, &name);
        }
    }
}

fn swap_pane(app: &mut App, delta: isize) {
    if app.busy {
        app.status = "busy — wait to switch".into();
        return;
    }
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.cycle_member(root, delta) {
            Ok(true) => {
                app.load_session_cards();
                app.expose_live();
                app.status = format!("session {}", app.session_id());
            }
            Ok(false) => app.status = "no other member".into(),
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) -> bool {
    if app.help {
        return match mouse.kind {
            MouseEventKind::ScrollUp => {
                help_scroll(app, -3);
                true
            }
            MouseEventKind::ScrollDown => {
                help_scroll(app, 3);
                true
            }
            _ => false,
        };
    }
    match mouse.kind {
        MouseEventKind::Moved => {
            if app.edge_drag.is_some() {
                apply_edge_drag(app, mouse.column, mouse.row);
                return true;
            }
            hover_at(app, mouse.column, mouse.row)
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.edge_drag.is_some() {
                apply_edge_drag(app, mouse.column, mouse.row);
                return true;
            }
            if let Some(sel) = app.selection.as_mut() {
                if !sel.finalized {
                    sel.drag(mouse.column, mouse.row);
                    return true;
                }
            }
            hover_at(app, mouse.column, mouse.row)
        }
        MouseEventKind::Down(MouseButton::Left) => {
            hover_at(app, mouse.column, mouse.row);
            mouse_down(app, mouse.column, mouse.row);
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if end_edge_drag(app) {
                return true;
            }
            mouse_up(app, mouse.column, mouse.row)
        }
        MouseEventKind::ScrollUp => {
            wheel_at(app, mouse.column, mouse.row, -1);
            true
        }
        MouseEventKind::ScrollDown => {
            wheel_at(app, mouse.column, mouse.row, 1);
            true
        }
        _ => false,
    }
}

fn mouse_down(app: &mut App, col: u16, row: u16) {
    let now = Instant::now();
    let double = app
        .last_click
        .as_ref()
        .is_some_and(|(t, c, r)| now.duration_since(*t) <= Duration::from_millis(400) && *c == col && *r == row);
    app.last_click = Some((now, col, row));

    if !matches!(app.hits.at(col, row), Some(HitKind::SplitEdge { .. })) {
        end_edge_drag(app);
    }

    match app.hits.at(col, row).cloned() {
        Some(HitKind::SplitEdge {
            path,
            gap,
            dir,
            sizes,
        }) => {
            app.selection = None;
            if double {
                if let Some(rail) = app.rail.as_mut() {
                    rail.equalize_split(path.as_deref());
                }
                if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
                    let _ = rail.persist(root);
                }
                app.status = "equal panes".into();
                return;
            }
            begin_edge_drag(app, col, row, path, gap, dir, sizes);
        }
        Some(HitKind::PasteChip(i)) => {
            app.selection = None;
            app.focus = Focus::Compose;
            if double {
                expand_paste_chip(app, i);
            }
        }
        Some(HitKind::Compose) => {
            app.selection = None;
            app.focus = Focus::Compose;
        }
        Some(HitKind::Pane(id)) => {
            if double {
                if let Some(word) = app.painted.word_at(&id, col, row) {
                    app.selection = Some(word);
                    if app.copy_on_select {
                        let _ = copy_selection_keep(app);
                    }
                    click_at(app, col, row);
                    return;
                }
            }
            app.selection = Some(select::Selection::begin(id, col, row));
            click_at(app, col, row);
        }
        _ => {
            app.selection = None;
            click_at(app, col, row);
        }
    }
}

fn begin_edge_drag(
    app: &mut App,
    col: u16,
    row: u16,
    path: Option<Vec<usize>>,
    gap: usize,
    dir: SplitDir,
    sizes: Vec<u16>,
) {
    let origin = match dir {
        SplitDir::Col => row,
        SplitDir::Row => col,
    };
    if let Some(rail) = app.rail.as_mut() {
        rail.seed_split_weights(path.as_deref(), &sizes);
    }
    let px_a = sizes.get(gap).copied().unwrap_or(1);
    let px_b = sizes.get(gap.saturating_add(1)).copied().unwrap_or(1);
    app.edge_drag = Some(EdgeDrag {
        path,
        gap,
        dir,
        origin,
        px_a,
        px_b,
    });
    app.status = "resize".into();
}

fn apply_edge_drag(app: &mut App, col: u16, row: u16) {
    let Some(drag) = app.edge_drag.clone() else {
        return;
    };
    let pos = match drag.dir {
        SplitDir::Col => row,
        SplitDir::Row => col,
    };
    let delta = i32::from(pos) - i32::from(drag.origin);
    if let Some(rail) = app.rail.as_mut() {
        rail.apply_split_gap(drag.path.as_deref(), drag.gap, drag.px_a, drag.px_b, delta);
    }
}

fn end_edge_drag(app: &mut App) -> bool {
    if app.edge_drag.take().is_none() {
        return false;
    }
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        let _ = rail.persist(root);
    }
    if app.status == "resize" {
        app.status = "idle".into();
    }
    true
}

fn mouse_up(app: &mut App, col: u16, row: u16) -> bool {
    let Some(sel) = app.selection.as_mut() else {
        return false;
    };
    if sel.finalized {
        return false;
    }
    sel.drag(col, row);
    if sel.was_just_click() {
        app.selection = None;
        return true;
    }
    if app.copy_on_select {
        let _ = copy_selection(app);
    } else {
        sel.finish();
    }
    true
}

fn copy_selection(app: &mut App) -> bool {
    if !write_selection(app) {
        return false;
    }
    app.selection = None;
    true
}

fn copy_selection_keep(app: &mut App) -> bool {
    write_selection(app)
}

fn write_selection(app: &mut App) -> bool {
    let Some(sel) = app.selection.as_ref() else {
        return false;
    };
    let Some(text) = app.painted.extract(sel) else {
        app.selection = None;
        return false;
    };
    if !clip::write_text(&text) {
        app.status = "clipboard write failed".into();
        return false;
    }
    app.toast("copied to clipboard");
    true
}

fn expand_paste_chip(app: &mut App, index: usize) {
    if index >= app.pastes.len() {
        return;
    }
    let chip = paste::chip_of(&app.pastes, index);
    let Some(start) = app.input.find(&chip) else {
        return;
    };
    let body = app.pastes[index].expand();
    app.input.replace_range(start..start + chip.len(), &body);
    app.cursor = start + body.len();
    app.pastes.remove(index);
}

fn handle_paste(app: &mut App, text: String) {
    if app.focused_is_pty() && app.focus == Focus::Compose {
        app.send_pty(text.as_bytes());
        return;
    }
    if app.focused_is_edit() && app.focus == Focus::Compose {
        app.send_edit(EditOp::Insert, &text);
        return;
    }
    if app.focus != Focus::Compose {
        return;
    }
    if text.trim().is_empty() {
        if let Some(image) = clip::read_image() {
            ingest_ask_image(app, image);
        }
        return;
    }
    ingest_ask_paste(app, text);
}

fn smart_paste(app: &mut App) {
    if app.focused_is_pty() && app.focus == Focus::Compose {
        if let Some(text) = clip::read_text() {
            app.send_pty(text.as_bytes());
        }
        return;
    }
    if app.focused_is_edit() && app.focus == Focus::Compose {
        if let Some(text) = clip::read_text() {
            app.send_edit(EditOp::Insert, &text);
        }
        return;
    }
    if app.focus != Focus::Compose {
        return;
    }
    if let Some(image) = clip::read_image() {
        ingest_ask_image(app, image);
        return;
    }
    if let Some(text) = clip::read_text() {
        ingest_ask_paste(app, text);
    }
}

fn ingest_ask_image(app: &mut App, image: clip::Image) {
    let incoming = paste::persist_image(image);
    if paste::try_expand_matching(&mut app.input, &mut app.cursor, &mut app.pastes, &incoming) {
        return;
    }
    app.pastes.push(incoming);
    let chip = paste::chip_of(&app.pastes, app.pastes.len() - 1);
    app.insert_str(&chip);
}

fn ingest_ask_paste(app: &mut App, text: String) {
    let Some(incoming) = paste::ingest_text(text.clone()) else {
        app.insert_str(&text);
        return;
    };
    if paste::try_expand_matching(&mut app.input, &mut app.cursor, &mut app.pastes, &incoming) {
        return;
    }
    app.pastes.push(incoming);
    let chip = paste::chip_of(&app.pastes, app.pastes.len() - 1);
    app.insert_str(&chip);
}

const DOUBLE_ESC: Duration = Duration::from_millis(400);

fn esc_again(last: Option<Instant>, now: Instant) -> bool {
    last.is_some_and(|t| now.duration_since(t) <= DOUBLE_ESC)
}

fn note_esc(app: &mut App, key: KeyEvent) -> bool {
    if key.code != KeyCode::Esc || !key.modifiers.is_empty() {
        return false;
    }
    if app.focused_is_pty() && app.focus == Focus::Compose {
        return false;
    }
    if app.focused_is_edit() && app.focus == Focus::Compose {
        return false;
    }
    let now = Instant::now();
    let armed = esc_again(app.last_esc, now);
    app.last_esc = Some(now);
    armed
}

fn try_double_esc(app: &mut App, key: KeyEvent, armed: bool) -> bool {
    if key.code != KeyCode::Esc || !key.modifiers.is_empty() {
        return false;
    }
    if app.focused_is_pty() && app.focus == Focus::Compose {
        return false;
    }
    if app.focused_is_edit() && app.focus == Focus::Compose {
        return false;
    }
    if app.help || app.settings || app.picker.is_some() || app.resize_mode || app.copy_mode {
        return false;
    }
    if app.focus != Focus::Compose {
        return false;
    }
    if app.selection.take().is_some() {
        return true;
    }
    if armed {
        app.clear_compose();
        return true;
    }
    if !app.input.is_empty() {
        app.status = "esc again to clear".into();
    }
    false
}

fn try_clipboard_keys(app: &mut App, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::SUPER);
    if !ctrl {
        return false;
    }
    match key.code {
        KeyCode::Char('c') | KeyCode::Char('C') => {
            if app
                .selection
                .as_ref()
                .is_some_and(|s| s.finalized || !s.was_just_click())
            {
                return copy_selection(app);
            }
            false
        }
        KeyCode::Char('v') | KeyCode::Char('V') => {
            smart_paste(app);
            true
        }
        _ => false,
    }
}

fn hit_key(kind: &HitKind) -> String {
    match kind {
        HitKind::Pane(id) => format!("pane:{id}"),
        HitKind::Member(id) => format!("member:{id}"),
        HitKind::Workspace(name) => format!("ws:{name}"),
        HitKind::Catalog => "catalog".into(),
        HitKind::Rail => "rail".into(),
        HitKind::Compose => "compose".into(),
        HitKind::PasteChip(i) => format!("chip:{i}"),
        HitKind::Tab(name) => format!("tab:{name}"),
        HitKind::TabAdd => "tabadd".into(),
        HitKind::SashPrev => "sash-".into(),
        HitKind::SashNext => "sash+".into(),
        HitKind::Picker(i) => format!("pick:{i}"),
        HitKind::SplitEdge { path, gap, .. } => match path {
            Some(p) => format!("edge:{p:?}:{gap}"),
            None => format!("edge:stage:{gap}"),
        },
    }
}

fn hover_at(app: &mut App, col: u16, row: u16) -> bool {
    app.pointer = Some((col, row));
    let Some(kind) = app.hits.at(col, row).cloned() else {
        return false;
    };
    if let HitKind::Pane(id) = &kind {
        app.scroll_under = Some(id.clone());
    }
    let key = hit_key(&kind);
    if app.hover.as_deref() == Some(key.as_str()) {
        return false;
    }
    app.hover = Some(key);
    match kind {
        HitKind::Pane(id) => {
            app.hover_member(&id, Focus::Compose);
        }
        HitKind::Member(id) => {
            app.hover_member(&id, Focus::Rail);
        }
        HitKind::Compose | HitKind::PasteChip(_) => app.focus = Focus::Compose,
        HitKind::Rail => app.focus = Focus::Rail,
        HitKind::Workspace(name) => {
            app.focus = Focus::Rail;
            if let Some(rail) = app.rail.as_mut() {
                rail.peek_row(RailKind::Workspace, &name);
            }
        }
        HitKind::Catalog => {
            app.focus = Focus::Rail;
            if let Some(rail) = app.rail.as_mut() {
                rail.kind = RailKind::Catalog;
            }
        }
        HitKind::Tab(_) | HitKind::TabAdd | HitKind::SashPrev | HitKind::SashNext => {}
        HitKind::Picker(_) => {}
        HitKind::SplitEdge { .. } => {}
    }
    true
}

fn click_at(app: &mut App, col: u16, row: u16) {
    let Some(kind) = app.hits.at(col, row).cloned() else {
        return;
    };
    match kind {
        HitKind::Rail => app.focus = Focus::Rail,
        HitKind::Catalog => {
            if let Some(rail) = app.rail.as_mut() {
                rail.kind = RailKind::Catalog;
                rail.idx = rail
                    .catalogs
                    .iter()
                    .position(|c| c == &rail.catalog)
                    .unwrap_or(0);
            }
            app.focus = Focus::Rail;
        }
        HitKind::Workspace(name) => {
            app.click_workspace(&name);
            app.focus = Focus::Rail;
        }
        HitKind::Member(id) => app.click_member(&id, Focus::Rail),
        HitKind::Tab(name) => {
            app.click_workspace(&name);
            app.focus = Focus::Compose;
        }
        HitKind::TabAdd => {
            if let Some(rail) = app.rail.as_mut() {
                rail.naming = Some(Naming::Session(String::new()));
            }
            app.focus = Focus::Rail;
        }
        HitKind::SashPrev => cycle_sash(app, -1),
        HitKind::SashNext => cycle_sash(app, 1),
        HitKind::Pane(id) => {
            if id.ends_with("::log") {
                app.view = MainView::Trajectory;
                app.focus = Focus::Compose;
            } else {
                app.view = MainView::Smith;
                app.click_member(&id, Focus::Compose);
            }
        }
        HitKind::Compose | HitKind::PasteChip(_) => app.focus = Focus::Compose,
        HitKind::SplitEdge { .. } => {}
        HitKind::Picker(i) => {
            if let Some(picker) = app.picker.as_mut() {
                picker.selected = i.min(picker.hits.len().saturating_sub(1));
            }
            app.accept_picker();
        }
    }
}

fn wheel_at(app: &mut App, col: u16, row: u16, dir: i32) {
    // Hover position is the truth. Some terminals report 0,0 or the
    // last click on the wheel event itself — do not clobber pointer.
    let (c, r) = app.pointer.unwrap_or((col, row));
    let kind = app
        .hits
        .at_scroll(c, r)
        .cloned()
        .or_else(|| app.hits.at_scroll(col, row).cloned());
    if let Some(HitKind::Pane(id)) = &kind {
        app.scroll_under = Some(id.clone());
    }
    let steps = 3 * dir;
    match kind {
        Some(HitKind::Pane(id)) => wheel_pane(app, &id, dir, steps),
        Some(HitKind::Member(_)) => {
            step_member(app, if dir < 0 { -1 } else { 1 });
        }
        Some(HitKind::Workspace(_)) => {
            step_workspace(app, if dir < 0 { -1 } else { 1 });
        }
        Some(HitKind::Rail) => match app.rail.as_ref().map(|r| r.kind) {
            Some(RailKind::Member) => step_member(app, if dir < 0 { -1 } else { 1 }),
            Some(RailKind::Workspace) => step_workspace(app, if dir < 0 { -1 } else { 1 }),
            _ => {
                if let Some(rail) = app.rail.as_mut() {
                    rail.move_idx(if dir < 0 { -1 } else { 1 });
                }
                app.focus = Focus::Rail;
            }
        },
        Some(HitKind::Tab(_)) | Some(HitKind::SashPrev) | Some(HitKind::SashNext) => {
            cycle_sash(app, if dir < 0 { -1 } else { 1 });
        }
        Some(HitKind::Picker(_)) => {
            if let Some(picker) = app.picker.as_mut() {
                if dir < 0 {
                    picker.selected = picker.selected.saturating_sub(1);
                } else if !picker.hits.is_empty() {
                    picker.selected = (picker.selected + 1).min(picker.hits.len() - 1);
                }
            }
        }
        Some(HitKind::Compose)
        | Some(HitKind::PasteChip(_))
        | Some(HitKind::Catalog)
        | Some(HitKind::TabAdd)
        | Some(HitKind::SplitEdge { .. })
        | None => {
            let id = app.scroll_under.clone().unwrap_or_else(|| app.scroll_key());
            app.bump_scroll(&id, steps);
        }
    }
}

fn wheel_pane(app: &mut App, id: &str, dir: i32, steps: i32) {
    if id.ends_with("::log") {
        app.bump_scroll(id, steps);
        return;
    }
    if app.member_is_pty(id) {
        if app.session_id() == id {
            let bytes: &[u8] = if dir < 0 { b"\x1b[A" } else { b"\x1b[B" };
            app.send_pty(bytes);
        }
        return;
    }
    if app.member_is_edit(id) {
        if app.session_id() == id {
            app.send_edit(if dir < 0 { EditOp::Up } else { EditOp::Down }, "");
        }
        return;
    }
    app.bump_scroll(id, steps);
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if let Some(naming) = app.rail.as_ref().and_then(|r| r.naming.clone()) {
        let buf = naming_buf(&naming);
        if app.keys.is_prefix(key) {
            app.prefix_armed = true;
            app.status = "prefix".into();
            return false;
        }
        return handle_naming(app, key, buf);
    }

    let double_esc = note_esc(app, key);

    if app.edge_drag.is_some() && key.code == KeyCode::Esc && key.modifiers.is_empty() {
        end_edge_drag(app);
        return false;
    }
    if app.resize_mode && handle_resize_key(app, key) {
        return false;
    }
    if app.copy_mode && handle_copy_key(app, key) {
        return false;
    }
    if app.help && handle_help_key(app, key) {
        return false;
    }

    let rail = app.focus == Focus::Rail;
    let pty = app.focused_is_pty() && app.focus == Focus::Compose;
    let edit = app.focused_is_edit() && app.focus == Focus::Compose;
    let picker = app.picker.is_some();
    let help = app.help || app.settings;
    let ok = |a: keys::Action| a.direct_ok(rail, pty, edit, picker, help);

    if app.prefix_armed {
        app.prefix_armed = false;
        if app.status == "prefix" {
            app.status = "idle".into();
        }
        if app.keys.is_prefix(key) {
            if pty {
                if let Some(bytes) = term::key_bytes(key) {
                    app.send_pty(&bytes);
                }
            }
            return false;
        }
        if let Some(act) = app.keys.resolve(key, true, |_| true) {
            return dispatch(app, act, key);
        }
        return false;
    }

    if app.keys.is_prefix(key) {
        app.prefix_armed = true;
        app.status = "prefix".into();
        return false;
    }

    if try_clipboard_keys(app, key) {
        return false;
    }

    if try_double_esc(app, key, double_esc) {
        return false;
    }

    if let Some(act) = app.keys.resolve(key, false, ok) {
        return dispatch(app, act, key);
    }

    if pty {
        if let Some(bytes) = term::key_bytes(key) {
            app.send_pty(&bytes);
        }
        return false;
    }
    if edit {
        return handle_edit_passthrough(app, key);
    }
    if rail || picker || help {
        return false;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char(ch), KeyModifiers::NONE | KeyModifiers::SHIFT) => app.insert(ch),
        (KeyCode::Backspace, _) => app.backspace(),
        (KeyCode::Left, _) if app.cursor > 0 => {
            let prev = app.input[..app.cursor]
                .chars()
                .next_back()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            app.cursor -= prev;
            app.refresh_picker();
        }
        (KeyCode::Right, _) if app.cursor < app.input.len() => {
            let next = app.input[app.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            app.cursor += next;
            app.refresh_picker();
        }
        _ => {}
    }
    false
}

fn dispatch(app: &mut App, act: keys::Action, key: KeyEvent) -> bool {
    use keys::Action::*;
    let act = act.canonical();
    match act {
        Detach => return true,
        Help => {
            app.help = !app.help;
            if app.help {
                app.help_view = HelpView::default();
            }
            app.settings = false;
        }
        Settings => {
            app.settings = !app.settings;
            app.help = false;
        }
        ReloadConfig => reload_config(app),
        Notify => app.status = "no notifications".into(),
        ToggleRail => {
            if app.rail.is_some() {
                app.focus = match app.focus {
                    Focus::Rail => Focus::Compose,
                    Focus::Compose => Focus::Rail,
                };
            }
        }
        WorkspacePicker => open_catalog_picker(app),
        Goto => open_goto_picker(app),
        NewWorkspace => start_naming(app, Naming::Catalog(String::new())),
        NewWorktree => app.status = "worktrees later".into(),
        RenameWorkspace => start_naming(app, Naming::RenameCatalog(String::new())),
        CloseWorkspace => close_catalog(app),
        NewTab => start_naming(app, Naming::Tab(String::new())),
        RenameTab => start_naming(app, Naming::RenameTab(String::new())),
        CloseTab => close_tab(app),
        NextSash => cycle_sash(app, 1),
        PrevSash => cycle_sash(app, -1),
        SwitchTab => {
            if let Some(n) = keys::digit_of(key) {
                switch_tab(app, n);
            }
        }
        FocusPaneLeft | NavigatePaneLeft => focus_pane_dir(app, NavDir::Left),
        FocusPaneDown | NavigatePaneDown => focus_pane_dir(app, NavDir::Down),
        FocusPaneUp | NavigatePaneUp => focus_pane_dir(app, NavDir::Up),
        FocusPaneRight | NavigatePaneRight => focus_pane_dir(app, NavDir::Right),
        SwapPaneLeft => swap_pane_dir(app, NavDir::Left),
        SwapPaneDown => swap_pane_dir(app, NavDir::Down),
        SwapPaneUp => swap_pane_dir(app, NavDir::Up),
        SwapPaneRight => swap_pane_dir(app, NavDir::Right),
        CyclePaneNext => swap_pane(app, 1),
        CyclePanePrev => swap_pane(app, -1),
        GrowPane => bump_weight(app, 1),
        ShrinkPane => bump_weight(app, -1),
        SplitVertical => split_pane(app, SplitDir::Row),
        SplitHorizontal => split_pane(app, SplitDir::Col),
        ClosePane => close_pane(app),
        Zoom => {
            app.zoom = !app.zoom;
            app.status = if app.zoom {
                "zoom".into()
            } else {
                "unzoom".into()
            };
        }
        ResizeMode => {
            app.resize_mode = !app.resize_mode;
            app.copy_mode = false;
            app.status = if app.resize_mode {
                "resize · hjkl · esc".into()
            } else {
                "resize off".into()
            };
        }
        CopyMode => {
            app.copy_mode = !app.copy_mode;
            app.resize_mode = false;
            app.status = if app.copy_mode {
                "copy · hjkl scroll · q".into()
            } else {
                "copy off".into()
            };
        }
        RenamePane => start_naming(app, Naming::RenamePane(String::new())),
        EditScrollback => edit_scrollback(app),
        Ask => app.submit_ask(),
        Strike => app.submit_strike(),
        Newline => app.insert('\n'),
        ClearCompose => app.clear_compose(),
        Fold => app.toggle_last_fold(),
        Verbosity => {
            app.verbosity = app.verbosity.next();
            app.status = format!("verbose {}", app.verbosity.label());
        }
        Mount => app.mount("clock", None),
        Unmount => app.unmount(None),
        Trajectory => {
            app.view = match app.view {
                MainView::Smith => MainView::Trajectory,
                MainView::Trajectory => MainView::Smith,
            };
            if app.view == MainView::Trajectory {
                app.reload_log();
                app.stick_focused();
            }
        }
        NewSession => start_naming(app, Naming::Session(String::new())),
        NewPty => start_naming(app, Naming::Pty(String::new())),
        NewEdit => start_naming(app, Naming::Edit(String::new())),
        NewClock => {
            if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
                match rail.create_clock(root) {
                    Ok(()) => {
                        app.mount("clock", None);
                        app.status = "clock member".into();
                    }
                    Err(err) => app.status = err.to_string(),
                }
            }
        }
        NewLog => {
            if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
                let of = rail.session.clone();
                match rail.create_log(root, &of) {
                    Ok(()) => {
                        app.load_session_cards();
                        app.expose_live();
                        app.status = format!("log {of}");
                    }
                    Err(err) => app.status = err.to_string(),
                }
            }
        }
        NewPlot => {
            if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
                let of = rail
                    .plot_of(&rail.session)
                    .or(rail.log_of(&rail.session))
                    .unwrap_or(rail.session.as_str())
                    .to_string();
                match rail.create_plot(root, &of) {
                    Ok(()) => {
                        app.load_session_cards();
                        app.expose_live();
                        app.status = format!("plot {of}");
                    }
                    Err(err) => app.status = err.to_string(),
                }
            }
        }
        PageUp => {
            let id = app.scroll_key();
            app.bump_scroll(&id, -10);
        }
        PageDown => {
            let id = app.scroll_key();
            app.bump_scroll(&id, 10);
        }
        FocusCompose => {
            app.help = false;
            app.settings = false;
            app.resize_mode = false;
            app.copy_mode = false;
            app.picker = None;
            app.focus = Focus::Compose;
        }
        RailWorkspaceUp | RailWorkspaceDown => {}
        RailCycle => {
            if let Some(rail) = app.rail.as_mut() {
                rail.cycle_kind();
            }
        }
        RailEnter => {
            if app.busy {
                app.status = "busy — wait to switch".into();
            } else if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
                match rail.apply_enter(root) {
                    Ok(true) => {
                        app.load_session_cards();
                        app.expose_live();
                        app.status = format!("session {}", app.session_id());
                    }
                    Ok(false) => {}
                    Err(err) => app.status = err.to_string(),
                }
            }
        }
        PickerUp => {
            if let Some(p) = app.picker.as_mut() {
                p.selected = p.selected.saturating_sub(1);
            }
        }
        PickerDown => {
            if let Some(p) = app.picker.as_mut() {
                if !p.hits.is_empty() {
                    p.selected = (p.selected + 1).min(p.hits.len() - 1);
                }
            }
        }
        PickerAccept => app.accept_picker(),
        PickerCancel => {
            app.picker = None;
            app.help = false;
            app.settings = false;
        }
    }
    false
}

fn naming_buf(naming: &Naming) -> String {
    match naming {
        Naming::Session(b)
        | Naming::Pty(b)
        | Naming::Edit(b)
        | Naming::Tab(b)
        | Naming::Catalog(b)
        | Naming::RenameTab(b)
        | Naming::RenameCatalog(b)
        | Naming::RenamePane(b) => b.clone(),
    }
}

fn start_naming(app: &mut App, naming: Naming) {
    if let Some(rail) = app.rail.as_mut() {
        rail.naming = Some(naming);
        app.focus = Focus::Rail;
    }
}

fn handle_resize_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.resize_mode = false;
            app.status = "resize off".into();
            true
        }
        KeyCode::Char('h') | KeyCode::Left => {
            bump_weight(app, -1);
            true
        }
        KeyCode::Char('l') | KeyCode::Right => {
            bump_weight(app, 1);
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            bump_weight(app, -1);
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            bump_weight(app, 1);
            true
        }
        _ => false,
    }
}

fn handle_help_key(app: &mut App, key: KeyEvent) -> bool {
    if app.help_view.search {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                app.help_view.search = false;
                true
            }
            (KeyCode::Enter, _) => {
                app.help_view.search = false;
                true
            }
            (KeyCode::Backspace, _) => {
                app.help_view.query.pop();
                app.help_view.scroll = 0;
                true
            }
            (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                app.help_view.query.clear();
                app.help_view.scroll = 0;
                true
            }
            (KeyCode::Char(ch), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                app.help_view.query.push(ch);
                app.help_view.scroll = 0;
                true
            }
            (KeyCode::Up, _) => {
                help_scroll(app, -1);
                true
            }
            (KeyCode::Down, _) => {
                help_scroll(app, 1);
                true
            }
            (KeyCode::PageUp, _) => {
                help_scroll(app, -8);
                true
            }
            (KeyCode::PageDown, _) => {
                help_scroll(app, 8);
                true
            }
            _ => false,
        }
    } else {
        match (key.code, key.modifiers) {
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                app.help_view.search = true;
                true
            }
            (KeyCode::Esc, _) | (KeyCode::Enter, _) => {
                app.help = false;
                app.help_view = HelpView::default();
                true
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => {
                help_scroll(app, 1);
                true
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => {
                help_scroll(app, -1);
                true
            }
            (KeyCode::PageDown, _) => {
                help_scroll(app, 8);
                true
            }
            (KeyCode::PageUp, _) => {
                help_scroll(app, -8);
                true
            }
            _ => false,
        }
    }
}

fn help_scroll(app: &mut App, delta: i32) {
    let max = app.help_view.max;
    let next = if delta < 0 {
        app.help_view
            .scroll
            .saturating_sub(delta.unsigned_abs() as u16)
    } else {
        app.help_view.scroll.saturating_add(delta as u16)
    };
    app.help_view.scroll = next.min(max);
}

fn handle_copy_key(app: &mut App, key: KeyEvent) -> bool {
    let id = app.scroll_key();
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.copy_mode = false;
            app.status = "copy off".into();
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.bump_scroll(&id, -1);
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.bump_scroll(&id, 1);
            true
        }
        KeyCode::Char('u') | KeyCode::PageUp => {
            app.bump_scroll(&id, -10);
            true
        }
        KeyCode::Char('d') | KeyCode::PageDown => {
            app.bump_scroll(&id, 10);
            true
        }
        KeyCode::Char('y') => {
            if !copy_selection(app) {
                app.status = "nothing selected".into();
            }
            true
        }
        _ => false,
    }
}

fn focus_pane_dir(app: &mut App, dir: NavDir) {
    let id = app.session_id();
    let next = app
        .hits
        .pane_area(&id)
        .and_then(|area| app.hits.nearest_pane(area, dir));
    if let Some(next) = next {
        app.click_member(&next, app.focus);
    } else {
        swap_pane(
            app,
            match dir {
                NavDir::Down | NavDir::Right => 1,
                NavDir::Up | NavDir::Left => -1,
            },
        );
    }
}

fn swap_pane_dir(app: &mut App, dir: NavDir) {
    let id = app.session_id();
    let Some(area) = app.hits.pane_area(&id) else {
        return;
    };
    let Some(other) = app.hits.nearest_pane(area, dir) else {
        app.status = "no pane that way".into();
        return;
    };
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.swap_with(root, &other) {
            Ok(true) => app.status = format!("swap {id} ↔ {other}"),
            Ok(false) => {}
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn close_pane(app: &mut App) {
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.close_pane(root) {
            Ok(true) => {
                app.load_session_cards();
                app.expose_live();
                app.status = format!("closed pane · {}", app.session_id());
            }
            Ok(false) => app.status = "last pane".into(),
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn close_tab(app: &mut App) {
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.close_tab(root) {
            Ok(true) => {
                app.load_session_cards();
                app.expose_live();
                app.status = format!(
                    "sash {}",
                    app.rail
                        .as_ref()
                        .map(|r| r.workspace.as_str())
                        .unwrap_or("")
                );
            }
            Ok(false) => app.status = "last sash".into(),
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn close_catalog(app: &mut App) {
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.close_catalog(root) {
            Ok(true) => {
                app.load_session_cards();
                app.expose_live();
                app.status = format!(
                    "catalog {}",
                    app.rail.as_ref().map(|r| r.catalog.as_str()).unwrap_or("")
                );
            }
            Ok(false) => app.status = "last catalog".into(),
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn switch_tab(app: &mut App, n: u8) {
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.switch_tab(root, n) {
            Ok(true) => {
                app.load_session_cards();
                app.expose_live();
                app.status = format!(
                    "sash {}",
                    app.rail
                        .as_ref()
                        .map(|r| r.workspace.as_str())
                        .unwrap_or("")
                );
            }
            Ok(false) => app.status = format!("no sash {n}"),
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn open_catalog_picker(app: &mut App) {
    let names = app
        .rail
        .as_ref()
        .map(|r| r.catalogs.clone())
        .unwrap_or_default();
    let hits = rank(&names, "", 24);
    app.picker = Some(PickerState {
        query: String::new(),
        hits,
        selected: 0,
        kind: PickerKind::Catalog,
    });
    app.status = "catalogs".into();
}

fn open_goto_picker(app: &mut App) {
    let mut names = Vec::new();
    if let Some(rail) = app.rail.as_ref() {
        names.extend(rail.workspaces.iter().cloned());
        for id in &rail.members {
            names.push(format!("{}/{}", rail.workspace, id));
        }
    }
    let hits = rank(&names, "", 24);
    app.picker = Some(PickerState {
        query: String::new(),
        hits,
        selected: 0,
        kind: PickerKind::Goto,
    });
    app.status = "goto".into();
}

fn reload_config(app: &mut App) {
    match Config::load_from(&app.config_path) {
        Ok((_, cfg)) => {
            theme::install(theme::Theme::from_config(&cfg.theme));
            app.keys = keys::Keymap::from_config(&cfg.keys);
            app.copy_on_select = cfg.ui.copy_on_select;
            app.status_auto_hide = cfg.ui.status_auto_hide;
            app.status_widgets = cfg.ui.status_widgets();
            app.context_window = cfg.ui.context_window;
            app.status = format!("reloaded {}", app.config_path.display());
        }
        Err(err) => app.status = err.to_string(),
    }
}

fn edit_scrollback(app: &mut App) {
    let id = app.session_id();
    let text = if app.member_is_pty(&id) {
        app.pty_screen
            .as_ref()
            .map(|s| s.lines.join("\n"))
            .unwrap_or_default()
    } else if app.member_is_edit(&id) {
        app.edit_buf
            .as_ref()
            .map(|b| b.text.clone())
            .unwrap_or_default()
    } else {
        app.cards
            .iter()
            .map(|c| match c {
                Card::User { text } | Card::Answer { text } | Card::Status { text } => text.clone(),
                Card::Thinking { text, .. } => text.clone(),
                Card::Step { timing, .. } => timing.compact(),
                Card::Strike { code, .. } => code.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.create_edit(root, "") {
            Ok(()) => {
                let edit_id = rail.session.clone();
                if let Ok(path) = root.edit_path(&edit_id) {
                    let _ = std::fs::write(path, text);
                }
                app.load_session_cards();
                app.expose_live();
                app.status = format!("scrollback {edit_id}");
            }
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn wrap_naming(kind: &Naming, buf: String) -> Naming {
    match kind {
        Naming::Session(_) => Naming::Session(buf),
        Naming::Pty(_) => Naming::Pty(buf),
        Naming::Edit(_) => Naming::Edit(buf),
        Naming::Tab(_) => Naming::Tab(buf),
        Naming::Catalog(_) => Naming::Catalog(buf),
        Naming::RenameTab(_) => Naming::RenameTab(buf),
        Naming::RenameCatalog(_) => Naming::RenameCatalog(buf),
        Naming::RenamePane(_) => Naming::RenamePane(buf),
    }
}

fn handle_naming(app: &mut App, key: KeyEvent, mut buf: String) -> bool {
    let kind = app.rail.as_ref().and_then(|r| r.naming.clone());
    let Some(kind) = kind else {
        return false;
    };
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => {
            if let Some(rail) = app.rail.as_mut() {
                rail.naming = None;
            }
        }
        (KeyCode::Enter, _) => {
            let name = buf.trim().to_string();
            finish_naming(app, &kind, &name);
        }
        (KeyCode::Backspace, _) => {
            buf.pop();
            if let Some(rail) = app.rail.as_mut() {
                rail.naming = Some(wrap_naming(&kind, buf));
            }
        }
        (KeyCode::Char(ch), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            buf.push(ch);
            if let Some(rail) = app.rail.as_mut() {
                rail.naming = Some(wrap_naming(&kind, buf));
            }
        }
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
        _ => {}
    }
    false
}

fn finish_naming(app: &mut App, kind: &Naming, name: &str) {
    let Some(root) = app.frame.clone() else {
        return;
    };
    let Some(rail) = app.rail.as_mut() else {
        return;
    };
    rail.naming = None;
    let empty_ok = matches!(kind, Naming::Edit(_) | Naming::Tab(_) | Naming::Catalog(_));
    if name.is_empty() && !empty_ok {
        return;
    }
    let result = match kind {
        Naming::Session(_) => rail
            .create_session(&root, name)
            .map(|_| format!("session {name}")),
        Naming::Pty(_) => rail.create_pty(&root, name).map(|_| format!("pty {name}")),
        Naming::Edit(_) => rail
            .create_edit(&root, name)
            .map(|_| format!("edit {}", rail.session)),
        Naming::Tab(_) => rail.create_tab(&root, name).map(|n| format!("sash {n}")),
        Naming::Catalog(_) => rail
            .create_catalog_front(&root, name)
            .map(|n| format!("catalog {n}")),
        Naming::RenameTab(_) => rail.rename_tab(&root, name).map(|n| format!("sash {n}")),
        Naming::RenameCatalog(_) => rail
            .rename_catalog(&root, name)
            .map(|n| format!("catalog {n}")),
        Naming::RenamePane(_) => {
            app.status = "rename pane: close and recreate".into();
            return;
        }
    };
    match result {
        Ok(msg) => {
            app.load_session_cards();
            app.expose_live();
            app.status = msg;
        }
        Err(err) => app.status = err.to_string(),
    }
}

fn handle_edit_passthrough(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Enter => app.send_edit(EditOp::Enter, ""),
        KeyCode::Backspace => app.send_edit(EditOp::Backspace, ""),
        KeyCode::Delete => app.send_edit(EditOp::Delete, ""),
        KeyCode::Left => app.send_edit(EditOp::Left, ""),
        KeyCode::Right => app.send_edit(EditOp::Right, ""),
        KeyCode::Up => app.send_edit(EditOp::Up, ""),
        KeyCode::Down => app.send_edit(EditOp::Down, ""),
        KeyCode::Home => app.send_edit(EditOp::Home, ""),
        KeyCode::End => app.send_edit(EditOp::End, ""),
        KeyCode::Char(ch)
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            let mut tmp = [0u8; 4];
            app.send_edit(EditOp::Insert, ch.encode_utf8(&mut tmp));
        }
        _ => {}
    }
    false
}

fn draw(frame: &mut Frame, app: &mut App) {
    app.painted.clear();
    app.pty_want.clear();
    app.compose_area = None;
    app.kitty_blit = None;
    let mut hits = Hits::default();
    let th = theme::t();
    frame.render_widget(Block::default().style(th.style(Face::Canvas)), frame.area());
    let shell = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(1)])
        .split(frame.area());
    let work = shell[0];
    draw_hint_bar(frame, shell[1], bottom_hints(app));
    let body = if app.rail.is_some() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(22), Constraint::Min(20)])
            .split(work);
        draw_rail(frame, app, cols[0], &mut hits);
        cols[1]
    } else {
        work
    };
    let picker_h = app
        .picker
        .as_ref()
        .map(|p| (p.hits.len() as u16 + 2).clamp(3, 10))
        .unwrap_or(0);
    let embed = app.rail.is_some();
    let pty_compose =
        (app.focused_is_pty() || app.focused_is_edit()) && app.focus == Focus::Compose;
    let input_h = if embed {
        0
    } else if pty_compose {
        1
    } else {
        (app.input.matches('\n').count() as u16 + 1).clamp(1, 8) + 2
    };
    let sash_h = if app.rail.is_some() { 1 } else { 0 };
    let status_h = if embed { 0 } else { 1 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(sash_h),
            Constraint::Min(6),
            Constraint::Length(picker_h),
            Constraint::Length(input_h),
            Constraint::Length(status_h),
        ])
        .split(body);

    if sash_h > 0 {
        draw_sashes(frame, app, chunks[0], &mut hits);
    }
    let main = chunks[1];
    let inner_w = main.width.saturating_sub(2);
    let lines = match app.view {
        MainView::Smith => render_cards(&app.cards, inner_w, app.verbosity, app.activity.as_ref()),
        MainView::Trajectory => render_trajectory(&app.log_events, inner_w),
    };
    let title = match (app.view, &app.rail) {
        (MainView::Trajectory, Some(r)) => format!("trajectory · {}", r.session),
        (MainView::Trajectory, None) => "trajectory".into(),
        (MainView::Smith, Some(r)) => r.member_label(&r.session),
        (MainView::Smith, None) => "smith".into(),
    };
    let stage = app
        .rail
        .as_ref()
        .map(|r| r.stage_members())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| vec![app.session_id()]);
    let tiles = app.rail.as_ref().and_then(|r| r.tiles.clone());
    let split = app.view == MainView::Smith && stage.len() > 1 && !app.zoom;
    if app.view == MainView::Smith && tiles.is_some() && stage.len() > 1 && !app.zoom {
        draw_tile(frame, main, app, tiles.as_ref().unwrap(), &mut hits, &[]);
    } else if split {
        let weights = app
            .rail
            .as_ref()
            .map(|r| r.weights.clone())
            .unwrap_or_default();
        let constraints = tile_constraints(&weights, stage.len());
        let panes = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(main);
        let sizes: Vec<u16> = panes.iter().map(|p| p.height).collect();
        for (i, id) in stage.iter().enumerate() {
            let focused = id == &app.session_id();
            hits.push(panes[i], HitKind::Pane(id.clone()));
            draw_member_pane(frame, panes[i], app, id, focused, &mut hits);
        }
        paint_split_edges(
            frame,
            app,
            &mut hits,
            SplitDir::Col,
            None,
            &panes,
            &sizes,
        );
    } else if app.view == MainView::Smith && app.focused_is_edit() {
        let id = app.session_id();
        hits.push(main, HitKind::Pane(id.clone()));
        let (edit_text, edit_cur) = app
            .edit_buf
            .as_ref()
            .map(|b| (b.text.clone(), b.cursor_row_col()))
            .unwrap_or_else(|| (String::new(), (0, 0)));
        draw_edit_pane(
            frame,
            main,
            &id,
            &edit_text,
            edit_cur,
            app.focus == Focus::Compose,
            app,
            &id,
        );
    } else if app.view == MainView::Smith && app.member_is_plot(&app.session_id()) {
        let id = app.session_id();
        hits.push(main, HitKind::Pane(id.clone()));
        draw_plot_pane(frame, main, app, &id, true);
    } else if app.view == MainView::Smith && app.focused_is_pty() {
        let id = app.session_id();
        hits.push(main, HitKind::Pane(id.clone()));
        let sel = app.selection.clone();
        let (inner, lines) = term::draw(
            frame,
            main,
            &id,
            app.pty_screen.as_ref(),
            app.focus == Focus::Compose,
            sel.as_ref(),
            &id,
        );
        note_pty_cell(app, &id, inner);
        app.painted.record(&id, inner, &lines);
    } else if embed && app.view == MainView::Smith {
        let key = app.scroll_key();
        hits.push(main, HitKind::Pane(key.clone()));
        draw_smith_pane(
            frame,
            main,
            app,
            title.trim(),
            &lines,
            &key,
            true,
            &mut hits,
        );
    } else {
        let key = app.scroll_key();
        hits.push(main, HitKind::Pane(key.clone()));
        draw_scroll_pane(frame, main, title.trim(), &lines, app, &key, true);
    }

    let picker_area = chunks[2];
    let compose_area = chunks[3];
    let status_area = chunks[4];

    if let Some(picker) = &app.picker {
        let th = theme::t();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(th.pane_border(true))
            .title(Span::styled(
                format!(" @{} ", picker.query),
                th.pane_title(true),
            ))
            .style(th.style(Face::PickerField));
        let inner = block.inner(picker_area);
        frame.render_widget(Clear, picker_area);
        frame.render_widget(block, picker_area);
        let view_h = inner.height as usize;
        let max = picker.hits.len().saturating_sub(view_h.max(1));
        let start = picker
            .selected
            .saturating_sub(view_h.saturating_sub(1))
            .min(max);
        let metrics = scroll::Metrics {
            offset: start as u16,
            max: max as u16,
            viewport: inner.height,
        };
        let text = scroll::content(inner, metrics);
        let items: Vec<ListItem> = picker
            .hits
            .iter()
            .enumerate()
            .skip(start)
            .take(view_h)
            .map(|(i, hit)| {
                let style = if i == picker.selected {
                    th.style(Face::PickerHitActive)
                } else {
                    th.style(Face::PickerHit)
                };
                ListItem::new(hit.path.clone()).style(style)
            })
            .collect();
        frame.render_widget(List::new(items), text);
        scroll::render(frame, inner, metrics, true);
        for (row, i) in (start..picker.hits.len().min(start + view_h)).enumerate() {
            hits.push(
                Rect::new(text.x, text.y.saturating_add(row as u16), text.width, 1),
                HitKind::Picker(i),
            );
        }
    }

    if !embed {
        if pty_compose {
            frame.render_widget(
                Paragraph::new(term::hint()).style(theme::t().style(Face::StatusInk)),
                compose_area,
            );
        } else {
            draw_compose(frame, compose_area, app, &mut hits);
        }
    }
    if status_area.height > 0 {
        draw_status_line(frame, status_area, app);
    }
    draw_paste_preview(frame, app);
    draw_toast(frame, app);
    if app.help {
        draw_help(frame, app);
    }
    if app.settings {
        draw_settings(frame, app);
    }
    app.hits = hits;
}

fn draw_help(frame: &mut Frame, app: &mut App) {
    let th = theme::t();
    let area = frame.area();
    let w = area.width.saturating_sub(4).min(76);
    let h = area.height.saturating_sub(2).min(24).max(10);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let box_area = Rect::new(x, y, w, h);
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(th.pane_border(true))
            .style(th.style(Face::PickerField)),
        box_area,
    );
    let inner = Rect::new(
        box_area.x.saturating_add(1),
        box_area.y.saturating_add(1),
        box_area.width.saturating_sub(2),
        box_area.height.saturating_sub(2),
    );
    if inner.height < 5 || inner.width < 16 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " keybinds",
            th.style(Face::PaneTitleFocus)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ))),
        chunks[0],
    );
    let close_label = if app.help_view.search {
        "esc back"
    } else {
        "esc close"
    };
    let close_w = close_label.len() as u16 + 2;
    if chunks[0].width > close_w + 10 {
        let chip = Rect::new(
            chunks[0].x + chunks[0].width.saturating_sub(close_w),
            chunks[0].y,
            close_w,
            1,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" {close_label} "),
                th.style(Face::TabActive),
            )),
            chip,
        );
    }

    let search = if app.help_view.search {
        Line::from(vec![
            Span::styled(
                " / ",
                th.style(Face::HintKey)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                app.help_view.query.as_str(),
                th.style(Face::HintLabel)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(Span::styled(
            " press / to filter by command or shortcut",
            th.style(Face::HintSep),
        ))
    };
    frame.render_widget(Paragraph::new(search), chunks[1]);

    let groups = keys::filter_help_groups(&app.keys.help_groups(), &app.help_view.query);
    let key_width = groups
        .iter()
        .flat_map(|(_, rows)| rows.iter().map(|(k, _)| k.chars().count()))
        .max()
        .unwrap_or(8)
        .min(28);
    let mut lines: Vec<Line> = Vec::new();
    if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            " no matching keybinds",
            th.style(Face::HintSep),
        )));
    } else {
        for (i, (name, rows)) in groups.iter().enumerate() {
            if i > 0 {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(Span::styled(
                format!(" {name}"),
                th.style(Face::PaneTitleFocus)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )));
            for (key, label) in rows {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {key:<key_width$} "),
                        th.style(Face::HintKey)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                    Span::styled(label.clone(), th.style(Face::HintLabel)),
                ]));
            }
        }
    }

    let body = chunks[2];
    let max = lines.len().saturating_sub(body.height as usize) as u16;
    app.help_view.max = max;
    app.help_view.scroll = app.help_view.scroll.min(max);
    let metrics = scroll::Metrics {
        offset: app.help_view.scroll,
        max,
        viewport: body.height,
    };
    let text = scroll::content(body, metrics);
    frame.render_widget(
        Paragraph::new(lines).scroll((app.help_view.scroll, 0)),
        text,
    );
    scroll::render(frame, body, metrics, true);

    let footer = if app.help_view.search {
        Line::from(vec![
            Span::styled(" filter ", th.style(Face::HintSep)),
            Span::styled("type/backspace", th.style(Face::HintLabel)),
            Span::styled(" · ", th.style(Face::HintSep)),
            Span::styled("clear ", th.style(Face::HintSep)),
            Span::styled("ctrl+u", th.style(Face::HintLabel)),
            Span::styled(" · ", th.style(Face::HintSep)),
            Span::styled("scroll ", th.style(Face::HintSep)),
            Span::styled("↑↓/pgup/pgdn", th.style(Face::HintLabel)),
            Span::styled(" · ", th.style(Face::HintSep)),
            Span::styled("back ", th.style(Face::HintSep)),
            Span::styled("esc", th.style(Face::HintLabel)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" search ", th.style(Face::HintSep)),
            Span::styled("/", th.style(Face::HintLabel)),
            Span::styled(" · ", th.style(Face::HintSep)),
            Span::styled("scroll ", th.style(Face::HintSep)),
            Span::styled("j/k/↑↓/pgup/pgdn", th.style(Face::HintLabel)),
            Span::styled(" · ", th.style(Face::HintSep)),
            Span::styled("close ", th.style(Face::HintSep)),
            Span::styled("esc/enter", th.style(Face::HintLabel)),
        ])
    };
    frame.render_widget(Paragraph::new(footer), chunks[3]);
}

fn draw_settings(frame: &mut Frame, app: &App) {
    let th = theme::t();
    let area = frame.area();
    let w = area.width.saturating_sub(8).min(64);
    let lines = vec![
        Line::from(Span::styled(
            format!(" config  {}", app.config_path.display()),
            th.style(Face::HintLabel),
        )),
        Line::from(Span::styled(
            format!(" prefix  {}", app.keys.prefix.display()),
            th.style(Face::HintLabel),
        )),
        Line::from(Span::styled(
            format!(" provider  {} · {}", app.provider_name, app.model),
            th.style(Face::HintLabel),
        )),
        Line::from(Span::styled(
            " reload   prefix+shift+r",
            th.style(Face::HintKey),
        )),
        Line::from(Span::styled(" keys     prefix+?", th.style(Face::HintKey))),
        Line::from(Span::styled(" esc      close", th.style(Face::HintKey))),
    ];
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(w).saturating_sub(2) / 2;
    let y = area.y + 2;
    let box_area = Rect::new(x, y, w, h);
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(th.pane_border(true))
                .title(Span::styled(" settings ", th.pane_title(true)))
                .style(th.style(Face::PickerField)),
        ),
        box_area,
    );
}

fn draw_edit_pane(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    title: &str,
    text: &str,
    cursor: (u16, u16),
    focused: bool,
    app: &mut App,
    id: &str,
) {
    let th = theme::t();
    let (row, col) = cursor;
    let mut lines: Vec<Line> = if text.is_empty() {
        vec![Line::from(Span::styled(
            " (empty) ",
            th.style(Face::EditEmpty),
        ))]
    } else {
        text.split('\n')
            .map(|line| Line::from(Span::styled(line.to_string(), th.style(Face::PaneField))))
            .collect()
    };
    if let Some(line) = lines.get_mut(row as usize) {
        if focused {
            *line = Line::from(Span::styled(
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>(),
                th.style(Face::EditCursor),
            ));
        }
    }
    let block = pane_block(&format!("{title} · edit"), focused);
    let inner = block.inner(area);
    let shown = select::apply_highlight(&lines, inner, app.selection.as_ref(), id);
    app.painted.record(id, inner, &lines);
    frame.render_widget(Paragraph::new(shown).block(block), area);
    if focused {
        let x = area.x.saturating_add(1).saturating_add(col);
        let y = area.y.saturating_add(1).saturating_add(row);
        if x < area.x.saturating_add(area.width.saturating_sub(1))
            && y < area.y.saturating_add(area.height.saturating_sub(1))
        {
            frame.set_cursor_position((x, y));
        }
    }
}

fn tile_constraints(weights: &[u16], n: usize) -> Vec<Constraint> {
    if n == 0 {
        return vec![Constraint::Ratio(1, 1)];
    }
    if weights.len() == n {
        let sum = u32::from(weights.iter().sum::<u16>()).max(1);
        weights
            .iter()
            .map(|w| Constraint::Ratio(u32::from(*w), sum))
            .collect()
    } else {
        let n = n as u32;
        (0..n).map(|_| Constraint::Ratio(1, n)).collect()
    }
}

fn draw_tile(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    app: &mut App,
    tile: &Tile,
    hits: &mut Hits,
    path: &[usize],
) {
    match tile {
        Tile::Leaf(id) => {
            hits.push(area, HitKind::Pane(id.clone()));
            let focused = id == &app.session_id();
            draw_member_pane(frame, area, app, id, focused, hits);
        }
        Tile::Split { dir, weights, kids } => {
            if kids.is_empty() {
                return;
            }
            let direction = match dir {
                SplitDir::Row => Direction::Horizontal,
                SplitDir::Col => Direction::Vertical,
            };
            let panes = Layout::default()
                .direction(direction)
                .constraints(tile_constraints(weights, kids.len()))
                .split(area);
            let sizes: Vec<u16> = panes
                .iter()
                .map(|p| match dir {
                    SplitDir::Col => p.height,
                    SplitDir::Row => p.width,
                })
                .collect();
            for (i, kid) in kids.iter().enumerate() {
                if let Some(pane) = panes.get(i) {
                    let mut child = path.to_vec();
                    child.push(i);
                    draw_tile(frame, *pane, app, kid, hits, &child);
                }
            }
            paint_split_edges(frame, app, hits, *dir, Some(path.to_vec()), &panes, &sizes);
        }
    }
}

fn paint_split_edges(
    frame: &mut Frame,
    app: &App,
    hits: &mut Hits,
    dir: SplitDir,
    path: Option<Vec<usize>>,
    panes: &[ratatui::layout::Rect],
    sizes: &[u16],
) {
    for gap in 0..panes.len().saturating_sub(1) {
        let edge = split_edge_rect(dir, panes[gap], panes[gap + 1]);
        if edge.width == 0 || edge.height == 0 {
            continue;
        }
        let hot = app.pointer.is_some_and(|(c, r)| hits::inside(edge, c, r))
            || app.edge_drag.as_ref().is_some_and(|d| {
                d.gap == gap && d.dir == dir && d.path == path
            });
        if hot {
            frame.render_widget(
                Paragraph::new("").style(theme::p().style(Face::ScrollThumb)),
                edge,
            );
        }
        hits.push(
            edge,
            HitKind::SplitEdge {
                path: path.clone(),
                gap,
                dir,
                sizes: sizes.to_vec(),
            },
        );
    }
}

fn draw_member_pane(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    app: &mut App,
    id: &str,
    focused: bool,
    hits: &mut Hits,
) {
    let label = app
        .rail
        .as_ref()
        .map(|r| r.member_label(id))
        .unwrap_or_else(|| id.to_string());
    if app.member_is_plot(id) {
        draw_plot_pane(frame, area, app, id, focused);
        return;
    }
    if app.member_is_log(id) {
        let events = if focused {
            &app.log_events
        } else {
            app.other_logs.get(id).map(Vec::as_slice).unwrap_or(&[])
        };
        let lines = render_trajectory(events, area.width.saturating_sub(2));
        draw_scroll_pane(frame, area, &label, &lines, app, id, focused);
        return;
    }
    if app.member_is_edit(id) {
        let (edit_text, edit_cur) = {
            let buf = if focused {
                app.edit_buf.as_ref()
            } else {
                app.other_edits.get(id)
            };
            buf.map(|b| (b.text.clone(), b.cursor_row_col()))
                .unwrap_or_else(|| (String::new(), (0, 0)))
        };
        draw_edit_pane(
            frame,
            area,
            &label,
            &edit_text,
            edit_cur,
            focused && app.focus == Focus::Compose,
            app,
            id,
        );
        return;
    }
    if app.member_is_pty(id) {
        let screen = if focused {
            app.pty_screen.as_ref()
        } else {
            app.other_ptys.get(id)
        };
        let sel = app.selection.clone();
        let (inner, lines) = term::draw(
            frame,
            area,
            &label,
            screen,
            focused && app.focus == Focus::Compose,
            sel.as_ref(),
            id,
        );
        note_pty_cell(app, id, inner);
        app.painted.record(id, inner, &lines);
        return;
    }
    let cards: &[Card] = if focused {
        &app.cards
    } else {
        app.other_cards.get(id).map(Vec::as_slice).unwrap_or(&[])
    };
    let lines = render_cards(
        cards,
        area.width.saturating_sub(2),
        app.verbosity,
        if focused { app.activity.as_ref() } else { None },
    );
    if app.member_is_session(id) {
        draw_smith_pane(frame, area, app, &label, &lines, id, focused, hits);
    } else {
        draw_scroll_pane(frame, area, &label, &lines, app, id, focused);
    }
}

fn pane_block(title: &str, focused: bool) -> Block<'static> {
    let pal = theme::p();
    let label = title.trim();
    Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(pal.pane_border(focused))
        .title(Span::styled(format!(" {label} "), pal.pane_title(focused)))
        .style(pal.bg())
}

fn draw_sashes(frame: &mut Frame, app: &App, area: ratatui::layout::Rect, hits: &mut Hits) {
    let pal = theme::p();
    frame.render_widget(
        Paragraph::new(" ".repeat(area.width as usize)).style(pal.style(Face::TabBar)),
        area,
    );
    let Some(rail) = &app.rail else {
        return;
    };
    let mut x = area.x;
    let y = area.y;
    for name in &rail.workspaces {
        let active = name == &rail.workspace;
        let label = format!(" {name} ");
        let w =
            (label.chars().count() as u16).min(area.width.saturating_sub(x.saturating_sub(area.x)));
        if w == 0 {
            break;
        }
        let style = if active {
            pal.tab_active()
        } else {
            pal.tab_idle()
        };
        let tab = Rect::new(x, y, w, 1);
        frame.render_widget(Paragraph::new(label).style(style), tab);
        hits.push(tab, HitKind::Tab(name.clone()));
        x = x.saturating_add(w).saturating_add(1);
        if x >= area.x + area.width {
            break;
        }
    }
    if x + 3 <= area.x + area.width {
        let add = Rect::new(x, y, 3, 1);
        frame.render_widget(Paragraph::new(" + ").style(pal.style(Face::TabAdd)), add);
        hits.push(add, HitKind::TabAdd);
    }
    draw_top_hints(frame, area, app, hits);
}

fn draw_scroll_pane(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    title: &str,
    lines: &[Line<'static>],
    app: &mut App,
    id: &str,
    focused: bool,
) {
    let block = pane_block(title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    paint_scrolled(frame, inner, lines, app, id, focused, Style::default(), true);
}

fn paint_scrolled(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    lines: &[Line<'static>],
    app: &mut App,
    id: &str,
    focused: bool,
    style: Style,
    wrap: bool,
) {
    let win = app.scroll_window(id, area, lines.len());
    paint_window(frame, area, lines, win, app, id, focused, style, wrap);
}

fn paint_window(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    lines: &[Line<'static>],
    win: scroll::Window,
    app: &mut App,
    id: &str,
    focused: bool,
    style: Style,
    wrap: bool,
) {
    let visible = win.slice(lines);
    app.painted.record(id, win.text, visible);
    let shown = select::apply_highlight(visible, win.text, app.selection.as_ref(), id);
    let mut para = Paragraph::new(shown).style(style);
    if wrap {
        para = para.wrap(Wrap { trim: false });
    }
    frame.render_widget(para, win.text);
    scroll::render(frame, area, win.metrics, focused);
}

fn draw_smith_pane(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    app: &mut App,
    title: &str,
    lines: &[Line<'static>],
    id: &str,
    focused: bool,
    hits: &mut Hits,
) {
    let pal = theme::p();
    let block = pane_block(title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let compose = focused && app.focus == Focus::Compose && app.view == MainView::Smith;
    let input_h = if compose {
        (app.input.matches('\n').count() as u16 + 3).clamp(3, 8)
    } else {
        0
    };
    let chip_h = u16::from(compose && app.busy);
    let status_h = u16::from(!app.status_auto_hide || focused);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),
            Constraint::Length(chip_h),
            Constraint::Length(input_h),
            Constraint::Length(status_h),
        ])
        .split(inner);
    hits.push(parts[0], HitKind::Pane(id.to_string()));
    paint_scrolled(frame, parts[0], lines, app, id, focused, pal.bg(), true);
    if chip_h == 1 {
        status::draw_progress(frame, parts[1], app);
    }
    if input_h > 0 {
        draw_compose(frame, parts[2], app, hits);
    }
    if status_h == 1 {
        status::draw(frame, parts[3], app, id);
    }
}

fn draw_compose(frame: &mut Frame, area: ratatui::layout::Rect, app: &mut App, hits: &mut Hits) {
    let pal = theme::p();
    let th = theme::t();
    app.compose_area = Some(area);
    hits.push(area, HitKind::Compose);
    let (text_area, origin) = if area.height >= 3 {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
            .border_style(th.style(Face::ComposeBorder))
            .style(pal.bg());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        (inner, (inner.x, inner.y))
    } else {
        (area, (area.x, area.y))
    };
    let mut spans = vec![Span::styled("❯ ", pal.accent_text())];
    let chips = paste::chip_spans(&app.input, &app.pastes);
    let mut pos = 0usize;
    let mut x = origin.0.saturating_add(2);
    let y = origin.1;
    for chip in &chips {
        if chip.start > pos {
            spans.push(Span::styled(
                app.input[pos..chip.start].to_string(),
                th.style(Face::ComposeInput),
            ));
            x = x.saturating_add(app.input[pos..chip.start].chars().count() as u16);
        }
        let label = app.input[chip.start..chip.end].to_string();
        let w = label.chars().count() as u16;
        spans.push(Span::styled(label, th.style(Face::ComposeChip)));
        hits.push(Rect::new(x, y, w.max(1), 1), HitKind::PasteChip(chip.index));
        x = x.saturating_add(w);
        pos = chip.end;
    }
    if pos < app.input.len() {
        spans.push(Span::styled(
            app.input[pos..].to_string(),
            th.style(Face::ComposeInput),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(pal.bg()),
        text_area,
    );
    if app.focus == Focus::Compose && !app.focused_is_pty() && !app.focused_is_edit() {
        let (cx, cy) = cursor_in(&app.input, app.cursor);
        frame.set_cursor_position((origin.0 + 2 + cx, origin.1 + cy));
    }
}

fn preview_paste_index(app: &App) -> Option<usize> {
    if let Some((col, row)) = app.pointer {
        if let Some(HitKind::PasteChip(i)) = app.hits.at(col, row) {
            return Some(*i);
        }
    }
    paste::chip_at(&app.input, app.cursor, &app.pastes).or_else(|| {
        if paste::chip_spans(&app.input, &app.pastes).is_empty() {
            None
        } else {
            app.pastes.len().checked_sub(1)
        }
    })
}

/// Tight card, dead-center on `anchor` (the composer), sitting just above it.
fn paste_preview_rect(anchor: Rect, line_widths: &[u16]) -> Rect {
    let inner = line_widths.iter().copied().max().unwrap_or(0).max(24);
    // Never more than half the composer — a full-bleed bar cannot look centered.
    let max_w = (anchor.width / 2).max(28).min(anchor.width.saturating_sub(2).max(1));
    let w = inner.saturating_add(4).min(max_w);
    let h = (line_widths.len() as u16).saturating_add(2).max(3);
    let x = anchor.x.saturating_add(anchor.width.saturating_sub(w) / 2);
    let y = anchor.y.saturating_sub(h);
    Rect::new(x, y, w, h)
}

/// Fit an image into a cell grid. Cells are ~1:2 (w:h) in pixels, so
/// `cols/rows ≈ 2 * px_w/px_h`. Clamp to the box; never stretch.
fn fit_image_cells(px_w: u32, px_h: u32, max_cols: u16, max_rows: u16) -> (u16, u16) {
    let px_w = px_w.max(1) as f64;
    let px_h = px_h.max(1) as f64;
    let cols_over_rows = 2.0 * px_w / px_h;
    let max_cols = max_cols.max(2) as f64;
    let max_rows = max_rows.max(1) as f64;
    let mut cols = max_cols;
    let mut rows = (cols / cols_over_rows).round().max(1.0);
    if rows > max_rows {
        rows = max_rows;
        cols = (rows * cols_over_rows).round().max(2.0);
        if cols > max_cols {
            cols = max_cols;
            rows = (cols / cols_over_rows).round().max(1.0);
        }
    }
    (cols as u16, rows as u16)
}

/// Larger centered card so the bitmap is visible, grok-build style.
fn image_preview_rect(anchor: Rect, px_w: u32, px_h: u32) -> Rect {
    let max_w = anchor.width.saturating_sub(4).min(88).max(36);
    let avail = anchor.y.saturating_sub(1).min(18).max(8);
    let chrome = 3u16;
    let max_img_rows = avail.saturating_sub(chrome).max(4);
    let (img_w, img_h) = fit_image_cells(px_w, px_h, max_w, max_img_rows);
    let w = img_w.max(24);
    let h = img_h.saturating_add(chrome);
    let x = anchor.x.saturating_add(anchor.width.saturating_sub(w) / 2);
    let y = anchor.y.saturating_sub(h);
    Rect::new(x, y, w, h)
}

fn wrap_preview_line(s: &str, width: u16) -> String {
    let w = width.max(8) as usize;
    let n = s.chars().count();
    if n <= w {
        return s.to_string();
    }
    format!("{}…", s.chars().take(w.saturating_sub(1)).collect::<String>())
}

fn draw_paste_preview(frame: &mut Frame, app: &mut App) {
    let Some(index) = preview_paste_index(app) else {
        return;
    };
    let Some(paste) = app.pastes.get(index) else {
        return;
    };
    let Some(compose) = app.compose_area else {
        return;
    };
    let n = paste::image_n(&app.pastes, index).max(1);
    if matches!(paste, paste::Paste::Image { .. }) {
        draw_image_preview(frame, app, compose, index, n);
        return;
    }
    let th = theme::t();
    let preview = paste::preview(paste, n);
    let cap = (compose.width / 2).saturating_sub(4).max(24);
    let mut lines: Vec<Line> = preview
        .lines
        .iter()
        .map(|s| {
            Line::from(Span::styled(
                format!(" {} ", wrap_preview_line(s, cap)),
                th.style(Face::PastePreview),
            ))
        })
        .collect();
    if preview.more > 0 {
        lines.push(Line::from(Span::styled(
            format!(" : ({} more lines)", preview.more),
            th.style(Face::PastePreviewMute),
        )));
    }
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {}", preview.footer_lead),
            th.style(Face::PastePreviewHint),
        ),
        Span::styled(preview.footer_or, th.style(Face::PastePreviewMute)),
        Span::styled(preview.footer_tail, th.style(Face::PastePreviewHint)),
    ]));
    let widths: Vec<u16> = lines
        .iter()
        .map(|line| select::line_text(line).chars().count() as u16)
        .collect();
    let box_area = paste_preview_rect(compose, &widths);
    if box_area.height == 0 || box_area.y >= compose.y && compose.y > 0 {
        return;
    }
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(th.pane_border(true))
                .style(th.style(Face::PastePreview)),
        ),
        box_area,
    );
}

fn draw_image_preview(
    frame: &mut Frame,
    app: &mut App,
    compose: Rect,
    index: usize,
    image_n: usize,
) {
    let Some(paste::Paste::Image { bytes, mime, path }) = app.pastes.get(index) else {
        return;
    };
    let bytes_len = bytes.len();
    let (px_w, px_h) = clip::dimensions(bytes).unwrap_or((16, 16));
    let kind = clip::kind_label(mime);
    let size = clip::fmt_size(bytes_len);
    let dim = clip::dimensions(bytes)
        .map(|(w, h)| format!("{w}x{h}"))
        .unwrap_or_else(|| "?x?".into());
    let name = path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("image-{image_n}"));
    let path_label = path.as_ref().map(|p| p.display().to_string());
    let box_area = image_preview_rect(compose, px_w, px_h);
    if box_area.width < 8 || box_area.height < 4 {
        return;
    }
    let th = theme::t();
    let title = format!(" Image #{image_n} — {kind} · {dim} · {size} · {name} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(th.pane_border(true))
        .title(Span::styled(
            wrap_preview_line(&title, box_area.width.saturating_sub(2)),
            th.style(Face::PastePreviewMute),
        ))
        .style(th.style(Face::PastePreview));
    let inner = block.inner(box_area);
    frame.render_widget(Clear, box_area);
    frame.render_widget(block, box_area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let path_h = u16::from(path_label.is_some());
    let img_h = inner.height.saturating_sub(path_h).max(1);
    let img_area = Rect::new(inner.x, inner.y, inner.width, img_h);
    if let Some(lines) = thumb::halfblocks(bytes, img_area.width, img_area.height) {
        frame.render_widget(Paragraph::new(lines), img_area);
    }
    let need_kitty = thumb::kitty_supported();
    let kitty_hit = app.kitty_cache.as_ref().and_then(|(len, c, r, png)| {
        (*len == bytes_len && *c == img_area.width && *r == img_area.height).then_some(png.clone())
    });
    let kitty_png = if need_kitty {
        kitty_hit.or_else(|| thumb::png_for_cells(bytes, img_area.width, img_area.height))
    } else {
        None
    };
    if let Some(p) = path_label {
        let y = inner.y.saturating_add(img_h);
        if y < inner.y.saturating_add(inner.height) {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(
                        " Path: {} ",
                        wrap_preview_line(&p, inner.width.saturating_sub(8))
                    ),
                    th.style(Face::PastePreviewMute),
                )),
                Rect::new(inner.x, y, inner.width, 1),
            );
        }
    }
    if let Some(png) = kitty_png {
        app.kitty_cache = Some((bytes_len, img_area.width, img_area.height, png.clone()));
        app.kitty_blit = Some(thumb::KittyBlit {
            area: img_area,
            png,
        });
    }
}

fn draw_plot_pane(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    app: &mut App,
    id: &str,
    focused: bool,
) {
    plot::draw(frame, area, app, id, focused);
}

fn draw_toast(frame: &mut Frame, app: &App) {
    let Some(toast) = &app.toast else {
        return;
    };
    if toast.until <= Instant::now() {
        return;
    }
    let th = theme::t();
    let area = frame.area();
    let label = format!(" {} ", toast.message);
    let w = (label.chars().count() as u16 + 2).min(area.width.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(w).saturating_sub(2);
    let y = area.y.saturating_add(area.height.saturating_sub(3));
    let box_area = Rect::new(x, y, w, 1);
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(Span::styled(label, th.style(Face::Toast))),
        box_area,
    );
}

fn hint_pair<'a>(key: &'a str, label: &'a str) -> Vec<Span<'a>> {
    let th = theme::t();
    vec![
        Span::styled(key, th.style(Face::HintKey)),
        Span::styled(":", th.style(Face::HintSep)),
        Span::styled(label, th.style(Face::HintLabel)),
    ]
}

fn bottom_hints(app: &App) -> Vec<(String, String)> {
    let k = &app.keys;
    if app.prefix_armed {
        return vec![
            (k.display(keys::Action::Detach), "detach".into()),
            (k.display(keys::Action::Help), "help".into()),
            (k.display(keys::Action::NextSash), "next sash".into()),
            (k.display(keys::Action::FocusPaneDown), "pane".into()),
            (k.prefix.display(), "literal".into()),
        ];
    }
    if app.focused_is_pty() && app.focus == Focus::Compose {
        vec![
            (k.display(keys::Action::ToggleRail), "rail".into()),
            ("keys".into(), "shell".into()),
            (k.display(keys::Action::Detach), "close".into()),
            (k.display(keys::Action::Help), "help".into()),
        ]
    } else if app.focused_is_edit() && app.focus == Focus::Compose {
        vec![
            (k.display(keys::Action::ToggleRail), "rail".into()),
            ("type".into(), "edit".into()),
            (k.display(keys::Action::Detach), "close".into()),
        ]
    } else {
        vec![
            (k.display(keys::Action::ToggleRail), "rail".into()),
            (k.display(keys::Action::Ask), "ask".into()),
            (k.display(keys::Action::Strike), "strike".into()),
            (k.display(keys::Action::NextSash), "sash".into()),
            (k.display(keys::Action::FocusPaneDown), "pane".into()),
            (k.display(keys::Action::Help), "help".into()),
        ]
    }
}

fn draw_hint_bar(frame: &mut Frame, area: ratatui::layout::Rect, pairs: Vec<(String, String)>) {
    let th = theme::t();
    frame.render_widget(Block::default().style(th.style(Face::HintBar)), area);
    let mut spans: Vec<Span> = Vec::new();
    for (i, (key, label)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  │  ", th.style(Face::HintSep)));
        }
        spans.extend(hint_pair(key.as_str(), label.as_str()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(th.style(Face::HintBar)),
        area,
    );
}

fn draw_top_hints(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, hits: &mut Hits) {
    let th = theme::t();
    let (idx, n, catalog) = app
        .rail
        .as_ref()
        .map(|r| {
            let i = r
                .workspaces
                .iter()
                .position(|w| w == &r.workspace)
                .unwrap_or(0)
                + 1;
            (i, r.workspaces.len().max(1), r.catalog.clone())
        })
        .unwrap_or((1, 1, "smith".into()));
    let mut pills: Vec<(String, Option<HitKind>)> = Vec::new();
    if n > 1 {
        pills.push((format!("{idx}/{n}"), None));
    }
    pills.push(("[<]".into(), Some(HitKind::SashPrev)));
    pills.push(("[>]".into(), Some(HitKind::SashNext)));
    pills.push((format!("[{catalog}]"), Some(HitKind::Catalog)));
    let total: u16 = pills
        .iter()
        .map(|(s, _)| s.chars().count() as u16 + 1)
        .sum::<u16>()
        .saturating_add(1);
    if total + 1 >= area.width {
        return;
    }
    let mut x = area.x + area.width.saturating_sub(total);
    for (label, kind) in pills {
        let w = label.chars().count() as u16;
        let rect = Rect::new(x, area.y, w, 1);
        frame.render_widget(Paragraph::new(label).style(th.style(Face::HintPill)), rect);
        if let Some(kind) = kind {
            hits.push(rect, kind);
        }
        x = x.saturating_add(w).saturating_add(1);
    }
}

fn draw_status_line(frame: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    status::draw(frame, area, app, &app.session_id());
}

fn draw_rail(frame: &mut Frame, app: &App, area: ratatui::layout::Rect, hits: &mut Hits) {
    let th = theme::t();
    hits.push(area, HitKind::Rail);
    frame.render_widget(Block::default().style(th.style(Face::Rail)), area);
    let Some(rail) = &app.rail else {
        return;
    };
    let focused = app.focus == Focus::Rail;
    let halves = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);
    let mut spaces: Vec<Line> = vec![Line::from(Span::styled(
        " spaces",
        th.style(Face::RailHeader),
    ))];
    if !rail.catalog.is_empty() {
        spaces.push(Line::from(Span::styled(
            format!(" {}", rail.catalog),
            th.style(Face::StatusInk),
        )));
    }
    push_dot_rows(
        &mut spaces,
        &rail.workspaces,
        &rail.workspace,
        rail.kind == RailKind::Workspace,
        rail.idx,
        focused,
        |_| Face::RailDotSession,
        |s| s.to_string(),
    );
    let mut space_line = 1;
    if !rail.catalog.is_empty() {
        hits.push(row_rect(halves[0], space_line), HitKind::Catalog);
        space_line += 1;
    }
    for name in &rail.workspaces {
        hits.push(
            row_rect(halves[0], space_line),
            HitKind::Workspace(name.clone()),
        );
        space_line += 1;
    }
    paint_rail_list(
        frame,
        halves[0],
        spaces,
        space_line.saturating_sub(1),
        focused,
    );

    let mut members: Vec<Line> = vec![Line::from(Span::styled(
        " members",
        th.style(Face::RailHeader),
    ))];
    push_dot_rows(
        &mut members,
        &rail.members,
        &rail.session,
        rail.kind == RailKind::Member,
        rail.idx,
        focused,
        |id| {
            if rail.ptys.iter().any(|p| p == id) {
                Face::RailDotPty
            } else if rail.edits.iter().any(|e| e == id) {
                Face::RailDotEdit
            } else if rail.member_is_log(id) {
                Face::RailDotLog
            } else if rail.member_is_plot(id) {
                Face::RailDotPlot
            } else if rail.member_is_clock(id) {
                Face::RailDotClock
            } else {
                Face::RailDotSession
            }
        },
        |id| rail.member_label(id),
    );
    for (i, id) in rail.members.iter().enumerate() {
        hits.push(row_rect(halves[1], i + 1), HitKind::Member(id.clone()));
    }
    match &rail.naming {
        Some(Naming::Session(buf)) => members.push(Line::from(Span::styled(
            format!(" new session: {buf}_"),
            th.style(Face::ComposePrompt),
        ))),
        Some(Naming::Pty(buf)) => members.push(Line::from(Span::styled(
            format!(" new pty: {buf}_"),
            th.style(Face::ComposePrompt),
        ))),
        Some(Naming::Edit(buf)) => members.push(Line::from(Span::styled(
            format!(" new edit: {buf}_"),
            th.style(Face::ComposePrompt),
        ))),
        Some(Naming::Tab(buf)) => members.push(Line::from(Span::styled(
            format!(" new sash: {buf}_"),
            th.style(Face::ComposePrompt),
        ))),
        Some(Naming::Catalog(buf)) => members.push(Line::from(Span::styled(
            format!(" new catalog: {buf}_"),
            th.style(Face::ComposePrompt),
        ))),
        Some(Naming::RenameTab(buf)) => members.push(Line::from(Span::styled(
            format!(" rename sash: {buf}_"),
            th.style(Face::ComposePrompt),
        ))),
        Some(Naming::RenameCatalog(buf)) => members.push(Line::from(Span::styled(
            format!(" rename catalog: {buf}_"),
            th.style(Face::ComposePrompt),
        ))),
        Some(Naming::RenamePane(buf)) => members.push(Line::from(Span::styled(
            format!(" rename pane: {buf}_"),
            th.style(Face::ComposePrompt),
        ))),
        None => {}
    }
    let member_keep = rail
        .members
        .iter()
        .position(|m| m == &rail.session)
        .unwrap_or(rail.idx)
        .saturating_add(1);
    paint_rail_list(frame, halves[1], members, member_keep, focused);
}

fn paint_rail_list(
    frame: &mut Frame,
    area: Rect,
    lines: Vec<Line<'static>>,
    keep: usize,
    focused: bool,
) {
    let view_h = area.height as usize;
    let max = lines.len().saturating_sub(view_h.max(1)) as u16;
    let keep = keep.min(lines.len().saturating_sub(1)) as u16;
    let mut offset = 0u16;
    if max > 0 {
        let last = offset.saturating_add(area.height.saturating_sub(1));
        if keep < offset {
            offset = keep;
        } else if keep > last {
            offset = keep.saturating_sub(area.height.saturating_sub(1));
        }
        offset = offset.min(max);
    }
    let metrics = scroll::Metrics {
        offset,
        max,
        viewport: area.height,
    };
    let text = scroll::content(area, metrics);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::t().style(Face::Rail))
            .scroll((offset, 0)),
        text,
    );
    scroll::render(frame, area, metrics, focused);
}

fn push_dot_rows(
    lines: &mut Vec<Line<'static>>,
    items: &[String],
    current: &str,
    active: bool,
    idx: usize,
    focused: bool,
    current_dot: impl Fn(&str) -> Face,
    display: impl Fn(&str) -> String,
) {
    let th = theme::t();
    if items.is_empty() {
        lines.push(Line::from(Span::styled("  —", th.style(Face::StatusInk))));
        return;
    }
    for (i, name) in items.iter().enumerate() {
        let on = name == current;
        let cursor = active && focused && i == idx;
        let dot = if on { "●" } else { "○" };
        let mut dot_style = if on {
            th.style(current_dot(name))
        } else {
            th.style(Face::RailDotIdle)
        };
        if cursor {
            dot_style = dot_style.bg(th.bg_of(Face::RailRowActive));
        }
        let label_style = if cursor {
            th.style(Face::RailRowActive)
        } else if on {
            th.style(Face::PaneField)
        } else {
            th.style(Face::RailRow)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {dot} "), dot_style),
            Span::styled(display(name), label_style),
        ]));
    }
}

fn card_from_event(event: &LogEvent) -> Option<Card> {
    match &event.body {
        EventBody::User { text } | EventBody::Ask { prompt: text, .. } => {
            Some(Card::User { text: text.clone() })
        }
        EventBody::Thinking { text, phase } => Some(Card::Thinking {
            text: text.clone(),
            folded: true,
            label: None,
            phase: activity::StepKind::from_phase_name(phase.as_deref()),
        }),
        EventBody::Step { n, timing } => Some(Card::Step {
            n: *n,
            timing: timing.clone(),
        }),
        EventBody::Strike {
            code,
            stdout,
            stderr,
            error,
            ok,
            ..
        } => Some(Card::Strike {
            code: code.clone(),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            error: error.clone(),
            ok: *ok,
            folded: true,
        }),
        EventBody::Answer { text, .. } => Some(Card::Answer { text: text.clone() }),
        EventBody::Status { text } => Some(Card::Status { text: text.clone() }),
        EventBody::Fiber { state } => Some(Card::Status {
            text: format!("fiber {state}"),
        }),
        EventBody::See { member, text } => Some(Card::Status {
            text: format!("see {member}\n{text}"),
        }),
    }
}

fn cursor_in(input: &str, cursor: usize) -> (u16, u16) {
    let before = &input[..cursor.min(input.len())];
    let y = before.matches('\n').count() as u16;
    let x = before.rsplit('\n').next().unwrap_or(before).chars().count() as u16;
    (x, y)
}

fn render_trajectory(events: &[LogEvent], width: u16) -> Vec<Line<'static>> {
    if events.is_empty() {
        return vec![field_line(
            " (empty log) ",
            theme::t().style(Face::EditEmpty),
            width,
        )];
    }
    events.iter().map(|e| trajectory_line(e, width)).collect()
}

fn trajectory_line(event: &LogEvent, width: u16) -> Line<'static> {
    let vis = if event.body.model_visible() { "v" } else { " " };
    let (kind, detail) = match &event.body {
        EventBody::User { text } => ("user", clip(text, 60)),
        EventBody::Ask { prompt, timing, .. } => {
            let extra = timing
                .as_ref()
                .filter(|t| !t.is_empty())
                .map(|t| format!(" {}", t.compact()))
                .unwrap_or_default();
            ("ask", format!("{}{extra}", clip(prompt, 40)))
        }
        EventBody::Thinking { text, phase } => (
            phase.as_deref().unwrap_or("think"),
            clip(text, 60),
        ),
        EventBody::Strike {
            code,
            ok,
            ms,
            timing,
            ..
        } => {
            let mark = if *ok { "ok" } else { "fail" };
            let time = timing
                .as_ref()
                .filter(|t| !t.is_empty())
                .map(|t| format!(" {}", t.compact()))
                .or_else(|| ms.map(|n| format!(" {n}ms")))
                .unwrap_or_default();
            ("strike", format!("{mark}{time} {}", clip(code, 40)))
        }
        EventBody::Answer { text, timing } => {
            let extra = timing
                .as_ref()
                .filter(|t| !t.is_empty())
                .map(|t| format!(" {}", t.compact()))
                .unwrap_or_default();
            ("answer", format!("{}{extra}", clip(text, 40)))
        }
        EventBody::Status { text } => ("status", clip(text, 60)),
        EventBody::Fiber { state } => ("fiber", state.clone()),
        EventBody::See { member, .. } => ("see", member.clone()),
        EventBody::Step { n, timing } => ("step", format!("{n} {}", timing.compact())),
    };
    let th = theme::t();
    let face = match &event.body {
        EventBody::Strike { ok: false, .. } => Face::MessageStrikeFail,
        EventBody::Strike { .. } => Face::MessageStrikeOk,
        EventBody::Ask { .. } | EventBody::User { .. } => Face::MessageUserInk,
        EventBody::Answer { .. } => Face::MessageAgentInk,
        EventBody::Thinking { phase, .. } => match phase.as_deref() {
            Some("decode") => Face::PlotDecode,
            Some("prefill") => Face::PlotPrefill,
            Some("tool") => Face::PlotTool,
            _ => Face::PlotThink,
        },
        EventBody::Step { .. } => Face::PlotPrefill,
        EventBody::See { .. } => Face::MessageSee,
        EventBody::Fiber { .. } | EventBody::Status { .. } => Face::MessageMute,
    };
    field_line(
        format!("{:>4} {vis} {kind:<6} {detail}", event.seq),
        th.style(face).bg(th.bg_of(Face::Trajectory)),
        width,
    )
}

fn clip(text: &str, max: usize) -> String {
    let one = text.lines().next().unwrap_or("").trim();
    let chars: Vec<char> = one.chars().collect();
    if chars.len() <= max {
        one.to_string()
    } else {
        format!("{}…", chars[..max].iter().collect::<String>())
    }
}

fn step_timing_lines(n: u32, timing: &crate::prof::Timing) -> Vec<Line<'static>> {
    let th = theme::t();
    let mut lines = vec![Line::from(Span::styled(
        format!(" ◆ Step {n}  {}", timing.compact()),
        th.style(Face::PlotMute),
    ))];
    let parts = [
        (
            activity::StepKind::Prefill,
            timing.ttft_ns.or(timing.prefill_ns),
            timing.tokens_in,
        ),
        (
            activity::StepKind::Think,
            timing.reason_ns,
            timing.tokens_reason,
        ),
        (
            activity::StepKind::Decode,
            timing.decode_ns,
            Some(timing.decode_tokens()).filter(|n| *n > 0),
        ),
        (activity::StepKind::Tool, timing.strike_ns, None),
    ];
    for (kind, dur, toks) in parts {
        let Some(ns) = dur.filter(|n| *n > 0) else {
            continue;
        };
        let mut label = format!("   {}  {}", kind.label(), crate::prof::fmt_ns(ns));
        if let Some(t) = toks {
            label.push_str(&format!("  {} tok", activity::fmt_tok(t)));
        }
        if kind == activity::StepKind::Decode {
            if let Some(r) = timing.tok_s {
                label.push_str(&format!("  {r:.1} tok/s"));
            }
        }
        lines.push(Line::from(Span::styled(label, th.style(kind.face(None)))));
    }
    lines
}

fn render_cards(
    cards: &[Card],
    width: u16,
    verbosity: activity::Verbosity,
    live: Option<&activity::Activity>,
) -> Vec<Line<'static>> {
    let th = theme::t();
    let quiet = verbosity == activity::Verbosity::Quiet;
    let full = verbosity == activity::Verbosity::Full;
    let mut lines = Vec::new();
    for card in cards {
        match card {
            Card::User { text } => {
                push_field(
                    &mut lines,
                    text,
                    th.style(Face::MessageUserInk),
                    width,
                    "❯ ",
                );
                lines.push(Line::from(""));
            }
            Card::Thinking {
                text,
                folded,
                label,
                phase,
            } => {
                if quiet {
                    continue;
                }
                let title = label
                    .clone()
                    .unwrap_or_else(|| phase.label().to_string());
                let step = activity::Step {
                    kind: *phase,
                    title,
                    body: text.clone(),
                    t0: std::time::Instant::now(),
                    dur: None,
                    ok: None,
                    tokens: 0,
                    out_lines: 0,
                };
                lines.extend(activity::step_line(&step, full && !*folded));
                if full && *folded {
                    if let Some(first) = text.lines().find(|l| !l.trim().is_empty()) {
                        push_field(
                            &mut lines,
                            &format!("{first}…"),
                            th.style(Face::StepMute),
                            width,
                            "    ",
                        );
                    }
                }
                lines.push(Line::from(""));
            }
            Card::Step { n, timing } => {
                if !full {
                    continue;
                }
                lines.extend(step_timing_lines(*n, timing));
                lines.push(Line::from(""));
            }
            Card::Strike {
                code,
                stdout,
                stderr,
                error,
                ok,
                folded: _,
            } => {
                if quiet {
                    continue;
                }
                let first = code.lines().next().unwrap_or("").trim().to_string();
                let step = activity::Step {
                    kind: activity::StepKind::Tool,
                    title: first,
                    body: code.clone(),
                    t0: std::time::Instant::now(),
                    dur: None,
                    ok: Some(*ok),
                    tokens: 0,
                    out_lines: stdout.lines().count() as u32,
                };
                lines.extend(activity::step_line(&step, full));
                if !full {
                    lines.push(Line::from(""));
                    continue;
                }
                if !stdout.is_empty() {
                    push_field(
                        &mut lines,
                        stdout,
                        th.style(Face::MessageAgentInk),
                        width,
                        "",
                    );
                }
                if !stderr.is_empty() {
                    push_field(&mut lines, stderr, th.style(Face::MessageMute), width, "");
                }
                if let Some(err) = error {
                    push_field(
                        &mut lines,
                        err,
                        th.style(Face::MessageStrikeFail),
                        width,
                        "",
                    );
                }
                lines.push(Line::from(""));
            }
            Card::Answer { text } => {
                push_field(&mut lines, text, th.style(Face::MessageAgentInk), width, "");
                lines.push(Line::from(""));
            }
            Card::Status { text } => {
                push_field(&mut lines, text, th.style(Face::MessageMute), width, "");
                lines.push(Line::from(""));
            }
        }
    }
    if !quiet {
        if let Some(act) = live {
            for s in &act.steps {
                if s.dur.is_none() {
                    lines.extend(activity::step_line(s, full));
                }
            }
        }
    }
    lines
}

fn field_line(text: impl Into<String>, style: Style, width: u16) -> Line<'static> {
    let mut text = text.into();
    let w = width.max(1) as usize;
    let n = text.chars().count();
    if n < w {
        text.push_str(&" ".repeat(w - n));
    }
    Line::from(Span::styled(text, style))
}

fn push_field(lines: &mut Vec<Line<'static>>, text: &str, style: Style, width: u16, prefix: &str) {
    let w = width.max(1) as usize;
    let body = if text.is_empty() { " " } else { text };
    let mut first = true;
    for raw in body.trim_end_matches('\n').lines() {
        let mut row = String::new();
        if first {
            row.push_str(prefix);
            first = false;
        }
        row.push_str(raw);
        let chars: Vec<char> = row.chars().collect();
        if chars.is_empty() {
            lines.push(field_line(String::new(), style, width));
            continue;
        }
        for chunk in chars.chunks(w) {
            let s: String = chunk.iter().collect();
            lines.push(field_line(s, style, width));
        }
    }
}

pub fn default_launch() -> Launch {
    Launch {
        store: None,
        root: None,
        session: None,
        workspace: None,
        catalog: None,
        hammer: default_hammer(),
        config: None,
        provider: None,
        model: None,
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

enum Slash {
    Mount { kind: String, slot: Option<String> },
    Unmount { id: Option<String> },
}

fn parse_slash(text: &str) -> Option<Slash> {
    let text = text.trim();
    if !text.starts_with('/') {
        return None;
    }
    let mut parts = text[1..].split_whitespace();
    match parts.next()? {
        "mount" => Some(Slash::Mount {
            kind: parts.next().unwrap_or("clock").to_string(),
            slot: parts.next().map(str::to_string),
        }),
        "unmount" => Some(Slash::Unmount {
            id: parts.next().map(str::to_string),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod slash_tests {
    use super::*;

    #[test]
    fn mount_defaults_to_clock() {
        match parse_slash("/mount").unwrap() {
            Slash::Mount { kind, slot } => {
                assert_eq!(kind, "clock");
                assert!(slot.is_none());
            }
            _ => panic!("expected mount"),
        }
    }

    #[test]
    fn mount_takes_kind_and_slot() {
        match parse_slash("/mount clock casing.status").unwrap() {
            Slash::Mount { kind, slot } => {
                assert_eq!(kind, "clock");
                assert_eq!(slot.as_deref(), Some("casing.status"));
            }
            _ => panic!("expected mount"),
        }
    }

    #[test]
    fn unmount_optional_id() {
        match parse_slash("/unmount").unwrap() {
            Slash::Unmount { id } => assert!(id.is_none()),
            _ => panic!("expected unmount"),
        }
        match parse_slash("/unmount dyn-1").unwrap() {
            Slash::Unmount { id } => assert_eq!(id.as_deref(), Some("dyn-1")),
            _ => panic!("expected unmount"),
        }
    }

    #[test]
    fn other_slashes_fall_through() {
        assert!(parse_slash("/help").is_none());
        assert!(parse_slash("mount clock").is_none());
    }

    #[test]
    fn trajectory_line_marks_visible_strike() {
        let ev = LogEvent {
            seq: 1,
            ts: 0,
            body: EventBody::Strike {
                code: "2+2".into(),
                stdout: String::new(),
                stderr: String::new(),
                error: None,
                ok: true,
                ms: Some(22),
                timing: None,
            },
        };
        let s: String = trajectory_line(&ev, 80)
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(s.contains('v'), "{s}");
        assert!(s.contains("strike"), "{s}");
        assert!(s.contains("22ms"), "{s}");
        assert!(s.contains("2+2"), "{s}");
    }

    #[test]
    fn paste_preview_is_centered_on_compose() {
        let compose = Rect::new(22, 20, 80, 1);
        let box_area = paste_preview_rect(compose, &[90, 36, 28]);
        assert!(
            box_area.width <= compose.width / 2,
            "card {} must be at most half of compose {}",
            box_area.width,
            compose.width
        );
        assert_eq!(
            box_area.x,
            compose.x + (compose.width - box_area.width) / 2
        );
        assert_eq!(box_area.y + box_area.height, compose.y);
        let mid_compose = compose.x + compose.width / 2;
        let mid_card = box_area.x + box_area.width / 2;
        assert!(
            mid_card.abs_diff(mid_compose) <= 1,
            "card mid {mid_card} vs compose mid {mid_compose}"
        );
    }

    #[test]
    fn image_preview_is_centered_and_taller() {
        let compose = Rect::new(22, 24, 80, 1);
        let box_area = image_preview_rect(compose, 1726, 1202);
        assert!(box_area.height >= 6, "image card should show pixels");
        let mid_compose = compose.x + compose.width / 2;
        let mid_card = box_area.x + box_area.width / 2;
        assert!(
            mid_card.abs_diff(mid_compose) <= 1,
            "card mid {mid_card} vs compose mid {mid_compose}"
        );
        assert_eq!(box_area.y + box_area.height, compose.y);
    }

    #[test]
    fn wide_image_stays_wide() {
        let (cols, rows) = fit_image_cells(1930, 316, 88, 15);
        assert!(
            cols > rows * 3,
            "1930x316 must be a wide strip, got {cols}x{rows}"
        );
        let compose = Rect::new(0, 24, 100, 1);
        let box_area = image_preview_rect(compose, 1930, 316);
        assert!(
            box_area.width > box_area.height,
            "card {}x{} should be landscape",
            box_area.width,
            box_area.height
        );
    }

    #[test]
    fn second_esc_inside_the_window_clears() {
        let t0 = Instant::now();
        assert!(!esc_again(None, t0));
        assert!(esc_again(Some(t0), t0 + Duration::from_millis(200)));
        assert!(!esc_again(Some(t0), t0 + Duration::from_millis(800)));
    }

    #[test]
    fn mouse_motion_is_not_an_action() {
        assert!(mouse_action(MouseEventKind::Down(MouseButton::Left)));
        assert!(mouse_action(MouseEventKind::ScrollUp));
        assert!(mouse_action(MouseEventKind::Up(MouseButton::Left)));
        assert!(mouse_action(MouseEventKind::Drag(MouseButton::Left)));
        assert!(!mouse_action(MouseEventKind::Moved));
    }

    #[test]
    fn unstick_from_bottom_not_from_zero() {
        let mut view = PaneView {
            scroll: 0,
            max: 40,
            stick: true,
        };
        apply_scroll(&mut view, -3);
        assert!(!view.stick);
        assert_eq!(view.scroll, 37);
        apply_scroll(&mut view, 10);
        assert!(view.stick);
        assert_eq!(view.scroll, 40);
        apply_scroll(&mut view, -1);
        assert_eq!(view.scroll, 39);
    }
}
