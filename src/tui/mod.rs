//! smith TUI: transcript blocks, ask worker, `@` file picker, casing rail.

mod picker;
mod rail;

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
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use ratatui::Terminal;

use crate::ask::{self, AskSink, HttpCompleter};
use crate::config::{Config, Provider};
use crate::frame::{self, Event as LogEvent, EventBody, FrameRoot};
use crate::serve::{self, Client, Spawn};
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
    Draft(String),
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
    fn on_draft(&mut self, text: &str) {
        let _ = self.tx.send(Ev::Draft(text.into()));
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
    scroll: u16,
    stick_bottom: bool,
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
    peer_session: Option<String>,
    peer_cards: Vec<Card>,
}

struct PickerState {
    query: String,
    hits: Vec<FileHit>,
    selected: usize,
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

    fn push_card(&mut self, card: Card) {
        // Serve appends the event log. The casing only projects.
        self.cards.push(card);
    }

    fn load_session_cards(&mut self) {
        self.cards.clear();
        self.reload_log();
        let start = self.log_events.len().saturating_sub(200);
        self.cards = self.log_events[start..]
            .iter()
            .filter_map(card_from_event)
            .collect();
        self.load_peer();
        self.stick_bottom = true;
    }

    fn load_peer(&mut self) {
        self.peer_cards.clear();
        self.peer_session = self.rail.as_ref().and_then(|r| r.peer_session());
        let Some(root) = &self.frame else {
            return;
        };
        let Some(peer) = &self.peer_session else {
            return;
        };
        if let Ok(events) = root.load_events(peer) {
            let start = events.len().saturating_sub(200);
            self.peer_cards = events[start..].iter().filter_map(card_from_event).collect();
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

    fn submit_ask(&mut self) {
        let text = self.input.trim_end().to_string();
        if text.is_empty() || self.busy {
            return;
        }
        if let Some(slash) = parse_slash(&text) {
            self.input.clear();
            self.cursor = 0;
            self.picker = None;
            self.dispatch_slash(slash);
            return;
        }
        self.input.clear();
        self.cursor = 0;
        self.picker = None;
        self.push_card(Card::User { text: text.clone() });
        self.busy = true;
        self.status = "thinking".into();
        self.stick_bottom = true;
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
        let text = self.input.trim_end().to_string();
        if text.is_empty() || self.busy {
            return;
        }
        self.input.clear();
        self.cursor = 0;
        self.picker = None;
        self.push_card(Card::User {
            text: format!("(strike)\n{text}"),
        });
        self.busy = true;
        self.status = "striking".into();
        self.stick_bottom = true;
        let _ = self.jobs.send(Job::Strike {
            session: self.session_id(),
            code: text,
        });
    }

    fn insert(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.refresh_picker();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
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
        if let Some((next, cur)) = insert_path(&self.input, self.cursor, &path) {
            self.input = next;
            self.cursor = cur;
        }
        self.picker = None;
    }

    fn apply(&mut self, ev: Ev) {
        match ev {
            Ev::Status(s) => self.status = s,
            Ev::Draft(text) => {
                if ask::extract_python(&text).is_none() {
                    self.push_card(Card::Thinking { text, folded: true });
                }
            }
            Ev::Strike {
                code,
                stdout,
                stderr,
                error,
                ok,
            } => {
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
                self.load_peer();
            }
            Ev::Answer(text) => {
                if !text.is_empty() {
                    self.push_card(Card::Answer { text });
                }
                self.busy = false;
                self.status = "idle".into();
                self.stick_bottom = true;
                self.reload_log();
            }
            Ev::Failed(text) => {
                self.push_card(Card::Status { text });
                self.busy = false;
                self.status = "error".into();
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
                "smith · {session_label} · {provider_name} · {model} · {} · Tab rail  Alt+L log  /mount clock  Alt+M/U  Ctrl+S strike  Ctrl+C close",
                cfg_path.display()
            ),
        }],
        input: String::new(),
        cursor: 0,
        status: "idle".into(),
        busy: false,
        scroll: 0,
        stick_bottom: true,
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
        peer_session: None,
        peer_cards: Vec::new(),
    };
    if app.frame.is_some() {
        app.load_session_cards();
        app.expose_live();
        if app.cards.is_empty() {
            app.cards.push(Card::Status {
                text: format!(
                    "smith · {} · {provider_name} · {model} · Tab rail  n new session  Enter expose",
                    app.session_id()
                ),
            });
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = event_loop(&mut terminal, &mut app);
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
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
    loop {
        while let Ok(ev) = app.events.try_recv() {
            app.apply(ev);
        }
        if last_tick.elapsed() >= Duration::from_millis(120) {
            app.tick = app.tick.wrapping_add(1);
            last_tick = Instant::now();
        }
        if last_inspect.elapsed() >= Duration::from_millis(400) {
            last_inspect = Instant::now();
            if let Some(sock) = &app.sock {
                if let Ok(mut c) = Client::connect(sock) {
                    if let Ok(report) = c.inspect() {
                        app.slot_status = report
                            .slots
                            .iter()
                            .find(|s| s.name == "casing.status")
                            .and_then(|s| s.text.clone());
                    }
                }
            }
        }
        terminal.draw(|frame| draw(frame, app))?;

        if !event::poll(Duration::from_millis(80))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if handle_key(app, key) {
                    return Ok(());
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    app.stick_bottom = false;
                    app.scroll = app.scroll.saturating_sub(3);
                }
                MouseEventKind::ScrollDown => {
                    app.scroll = app.scroll.saturating_add(3);
                }
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
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

fn swap_pane(app: &mut App) {
    if app.busy {
        app.status = "busy — wait to switch".into();
        return;
    }
    if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
        match rail.focus_peer(root) {
            Ok(true) => {
                app.load_session_cards();
                app.expose_live();
                app.status = format!("session {}", app.session_id());
            }
            Ok(false) => app.status = "no peer pane".into(),
            Err(err) => app.status = err.to_string(),
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if let Some(Naming::Session(buf)) = app.rail.as_ref().and_then(|r| r.naming.clone()) {
        return handle_naming(app, key, buf);
    }

    if let Some(picker) = app.picker.as_mut() {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                app.picker = None;
                return false;
            }
            (KeyCode::Up, _) => {
                picker.selected = picker.selected.saturating_sub(1);
                return false;
            }
            (KeyCode::Down, _) => {
                if !picker.hits.is_empty() {
                    picker.selected = (picker.selected + 1).min(picker.hits.len() - 1);
                }
                return false;
            }
            (KeyCode::Tab, _) | (KeyCode::Enter, _) => {
                app.accept_picker();
                return false;
            }
            _ => {}
        }
    }

    if app.rail.is_some() && matches!((key.code, key.modifiers), (KeyCode::Tab, _)) {
        app.focus = match app.focus {
            Focus::Rail => Focus::Compose,
            Focus::Compose => Focus::Rail,
        };
        return false;
    }

    if app.focus == Focus::Rail {
        return handle_rail_key(app, key);
    }

    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
        (KeyCode::Esc, _) => {}
        (KeyCode::Enter, KeyModifiers::NONE) => app.submit_ask(),
        (KeyCode::Char('s'), KeyModifiers::CONTROL) => app.submit_strike(),
        (KeyCode::Char('j'), KeyModifiers::CONTROL) | (KeyCode::Enter, KeyModifiers::SHIFT) => {
            app.insert('\n');
        }
        (KeyCode::Char('.'), KeyModifiers::ALT) => app.toggle_last_fold(),
        (KeyCode::Char('m'), KeyModifiers::ALT) => app.mount("clock", None),
        (KeyCode::Char('u'), KeyModifiers::ALT) => app.unmount(None),
        (KeyCode::Char('l'), KeyModifiers::ALT) => {
            app.view = match app.view {
                MainView::Smith => MainView::Trajectory,
                MainView::Trajectory => MainView::Smith,
            };
            if app.view == MainView::Trajectory {
                app.reload_log();
                app.stick_bottom = true;
            }
        }
        (KeyCode::Char('['), KeyModifiers::ALT) => cycle_sash(app, -1),
        (KeyCode::Char(']'), KeyModifiers::ALT) => cycle_sash(app, 1),
        (KeyCode::Char('j'), KeyModifiers::ALT) | (KeyCode::Char('k'), KeyModifiers::ALT) => {
            swap_pane(app);
        }
        (KeyCode::PageUp, _) => {
            app.stick_bottom = false;
            app.scroll = app.scroll.saturating_sub(10);
        }
        (KeyCode::PageDown, _) => app.scroll = app.scroll.saturating_add(10),
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

fn handle_naming(app: &mut App, key: KeyEvent, mut buf: String) -> bool {
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => {
            if let Some(rail) = app.rail.as_mut() {
                rail.naming = None;
            }
        }
        (KeyCode::Enter, _) => {
            let name = buf.trim().to_string();
            if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
                rail.naming = None;
                if !name.is_empty() {
                    match rail.create_session(root, &name) {
                        Ok(()) => {
                            app.load_session_cards();
                            app.expose_live();
                            app.status = format!("session {name}");
                        }
                        Err(err) => app.status = err.to_string(),
                    }
                }
            }
        }
        (KeyCode::Backspace, _) => {
            buf.pop();
            if let Some(rail) = app.rail.as_mut() {
                rail.naming = Some(Naming::Session(buf));
            }
        }
        (KeyCode::Char(ch), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            buf.push(ch);
            if let Some(rail) = app.rail.as_mut() {
                rail.naming = Some(Naming::Session(buf));
            }
        }
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
        _ => {}
    }
    false
}

fn handle_rail_key(app: &mut App, key: KeyEvent) -> bool {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
        (KeyCode::Esc, _) => app.focus = Focus::Compose,
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
            if let Some(rail) = app.rail.as_mut() {
                rail.move_idx(-1);
            }
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
            if let Some(rail) = app.rail.as_mut() {
                rail.move_idx(1);
            }
        }
        (KeyCode::Left, _) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
            if let Some(rail) = app.rail.as_mut() {
                rail.kind = match rail.kind {
                    RailKind::Member => RailKind::Workspace,
                    RailKind::Workspace => RailKind::Catalog,
                    RailKind::Catalog => RailKind::Catalog,
                };
                rail.reclamp();
            }
        }
        (KeyCode::Right, _) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
            if let Some(rail) = app.rail.as_mut() {
                rail.kind = match rail.kind {
                    RailKind::Catalog => RailKind::Workspace,
                    RailKind::Workspace => RailKind::Member,
                    RailKind::Member => RailKind::Member,
                };
                rail.reclamp();
            }
        }
        (KeyCode::Char('['), _) => {
            if let Some(rail) = app.rail.as_mut() {
                rail.cycle_kind();
            }
        }
        (KeyCode::Char('n'), KeyModifiers::NONE) => {
            if let Some(rail) = app.rail.as_mut() {
                rail.naming = Some(Naming::Session(String::new()));
            }
        }
        (KeyCode::Char('m'), KeyModifiers::NONE) => app.mount("clock", None),
        (KeyCode::Char('u'), KeyModifiers::NONE) => app.unmount(None),
        (KeyCode::Enter, _) => {
            if app.busy {
                app.status = "busy — wait to switch".into();
                return false;
            }
            if let (Some(root), Some(rail)) = (&app.frame, app.rail.as_mut()) {
                match rail.apply_enter(root) {
                    Ok(switched) => {
                        if switched {
                            app.load_session_cards();
                            app.expose_live();
                            app.status = format!("session {}", app.session_id());
                        }
                    }
                    Err(err) => app.status = err.to_string(),
                }
            }
        }
        _ => {}
    }
    false
}

fn draw(frame: &mut Frame, app: &App) {
    let body = if app.rail.is_some() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(24), Constraint::Min(20)])
            .split(frame.area());
        draw_rail(frame, app, cols[0]);
        cols[1]
    } else {
        frame.area()
    };
    let picker_h = app
        .picker
        .as_ref()
        .map(|p| (p.hits.len() as u16 + 2).clamp(3, 10))
        .unwrap_or(0);
    let input_h = (app.input.matches('\n').count() as u16 + 1).clamp(1, 8) + 2;
    let sash_h = if app.rail.is_some() { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(sash_h),
            Constraint::Min(6),
            Constraint::Length(picker_h),
            Constraint::Length(input_h),
            Constraint::Length(1),
        ])
        .split(body);

    if sash_h > 0 {
        draw_sashes(frame, app, chunks[0]);
    }
    let main = chunks[1];
    let lines = match app.view {
        MainView::Smith => render_cards(&app.cards),
        MainView::Trajectory => render_trajectory(&app.log_events),
    };
    let title = match (app.view, &app.rail) {
        (MainView::Trajectory, Some(r)) => format!(" trajectory · {} · Alt+L ", r.session),
        (MainView::Trajectory, None) => " trajectory · Alt+L ".into(),
        (MainView::Smith, Some(r)) => format!(" smith · {} ", r.session),
        (MainView::Smith, None) => " smith ".into(),
    };
    let split = app.view == MainView::Smith && app.peer_session.is_some();
    if split {
        let halves = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(main);
        draw_scroll_pane(frame, halves[0], &title, &lines, app);
        let peer = app.peer_session.as_deref().unwrap_or("peer");
        let peer_lines = render_cards(&app.peer_cards);
        draw_scroll_pane(frame, halves[1], &format!(" {peer} "), &peer_lines, app);
    } else {
        draw_scroll_pane(frame, main, &title, &lines, app);
    }

    let picker_area = chunks[2];
    let compose_area = chunks[3];
    let status_area = chunks[4];

    if let Some(picker) = &app.picker {
        let items: Vec<ListItem> = picker
            .hits
            .iter()
            .enumerate()
            .map(|(i, hit)| {
                let style = if i == picker.selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                ListItem::new(hit.path.clone()).style(style)
            })
            .collect();
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" @{} ", picker.query)),
        );
        frame.render_widget(Clear, picker_area);
        frame.render_widget(list, picker_area);
    }

    let compose = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(" ask "));
    frame.render_widget(compose, compose_area);
    let (cx, cy) = cursor_in(&app.input, app.cursor);
    frame.set_cursor_position((compose_area.x + cx + 1, compose_area.y + cy + 1));

    let spin = if app.busy {
        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"][(app.tick as usize) % 8]
    } else {
        "·"
    };
    let focus = match app.focus {
        Focus::Rail => "rail",
        Focus::Compose => "ask",
    };
    frame.render_widget(
        Paragraph::new(Line::from(format!(
            " {spin} {} · {} · {} · {}{} ",
            app.status,
            if app.view == MainView::Trajectory {
                "log"
            } else {
                focus
            },
            app.provider_name,
            app.model,
            app.slot_status
                .as_deref()
                .map(|t| format!(" · {t}"))
                .unwrap_or_default()
        ))),
        status_area,
    );
}

fn draw_sashes(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(rail) = &app.rail else {
        return;
    };
    let tabs: String = if rail.workspaces.is_empty() {
        " (no sashes) ".into()
    } else {
        rail.workspaces
            .iter()
            .map(|w| {
                if w == &rail.workspace {
                    format!("[{w}]")
                } else {
                    format!(" {w} ")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {tabs}  Alt+[ ]"),
            Style::default().fg(Color::Yellow),
        ))),
        area,
    );
}

fn draw_scroll_pane(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    title: &str,
    lines: &[Line<'static>],
    app: &App,
) {
    let view_h = area.height.saturating_sub(2);
    let max_scroll = lines.len().saturating_sub(view_h as usize) as u16;
    let scroll = if app.stick_bottom {
        max_scroll
    } else {
        app.scroll.min(max_scroll)
    };
    let widget = Paragraph::new(lines.to_vec())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title.to_string()),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(widget, area);
}

fn draw_rail(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(rail) = &app.rail else {
        return;
    };
    let mut lines: Vec<Line> = Vec::new();
    let focused = app.focus == Focus::Rail;
    push_rail_section(
        &mut lines,
        "catalogs",
        &rail.catalogs,
        &rail.catalog,
        rail.kind == RailKind::Catalog,
        rail.idx,
        focused,
    );
    push_rail_section(
        &mut lines,
        "workspaces",
        &rail.workspaces,
        &rail.workspace,
        rail.kind == RailKind::Workspace,
        rail.idx,
        focused,
    );
    push_rail_section(
        &mut lines,
        "members",
        &rail.members,
        &rail.session,
        rail.kind == RailKind::Member,
        rail.idx,
        focused,
    );
    if let Some(Naming::Session(buf)) = &rail.naming {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(" new session: {buf}_"),
            Style::default().fg(Color::Cyan),
        )));
    }
    let title = if focused { " rail " } else { " rail · Tab " };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn push_rail_section(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    items: &[String],
    current: &str,
    active: bool,
    idx: usize,
    focused: bool,
) {
    let style = if active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    lines.push(Line::from(Span::styled(format!(" {label}"), style)));
    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            "  —",
            Style::default().fg(Color::DarkGray),
        )));
        return;
    }
    for (i, name) in items.iter().enumerate() {
        let mark = if name == current { "●" } else { " " };
        let cursor = active && focused && i == idx;
        let body = format!(" {mark} {name}");
        let row = if cursor {
            Style::default().add_modifier(Modifier::REVERSED)
        } else if name == current {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(body, row)));
    }
}

fn card_from_event(event: &LogEvent) -> Option<Card> {
    match &event.body {
        EventBody::User { text } | EventBody::Ask { prompt: text, .. } => {
            Some(Card::User { text: text.clone() })
        }
        EventBody::Thinking { text } => Some(Card::Thinking {
            text: text.clone(),
            folded: true,
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
        EventBody::Answer { text } => Some(Card::Answer { text: text.clone() }),
        EventBody::Status { text } => Some(Card::Status { text: text.clone() }),
        EventBody::Fiber { state } => Some(Card::Status {
            text: format!("fiber {state}"),
        }),
    }
}

fn cursor_in(input: &str, cursor: usize) -> (u16, u16) {
    let before = &input[..cursor.min(input.len())];
    let y = before.matches('\n').count() as u16;
    let x = before.rsplit('\n').next().unwrap_or(before).chars().count() as u16;
    (x, y)
}

fn render_trajectory(events: &[LogEvent]) -> Vec<Line<'static>> {
    if events.is_empty() {
        return vec![Line::from(Span::styled(
            " (empty log) ",
            Style::default().fg(Color::DarkGray),
        ))];
    }
    events.iter().map(trajectory_line).collect()
}

fn trajectory_line(event: &LogEvent) -> Line<'static> {
    let vis = if event.body.model_visible() { "v" } else { " " };
    let (kind, detail) = match &event.body {
        EventBody::User { text } => ("user", clip(text, 60)),
        EventBody::Ask { prompt, .. } => ("ask", clip(prompt, 60)),
        EventBody::Thinking { text } => ("think", clip(text, 60)),
        EventBody::Strike { code, ok, ms, .. } => {
            let mark = if *ok { "ok" } else { "fail" };
            let time = ms.map(|n| format!(" {n}ms")).unwrap_or_default();
            ("strike", format!("{mark}{time} {}", clip(code, 40)))
        }
        EventBody::Answer { text } => ("answer", clip(text, 60)),
        EventBody::Status { text } => ("status", clip(text, 60)),
        EventBody::Fiber { state } => ("fiber", state.clone()),
    };
    let color = match &event.body {
        EventBody::Strike { ok: false, .. } => Color::Red,
        EventBody::Strike { .. } => Color::Green,
        EventBody::Ask { .. } | EventBody::User { .. } => Color::Cyan,
        EventBody::Answer { .. } => Color::White,
        EventBody::Thinking { .. } => Color::Yellow,
        EventBody::Fiber { .. } | EventBody::Status { .. } => Color::DarkGray,
    };
    Line::from(Span::styled(
        format!("{:>4} {vis} {kind:<6} {detail}", event.seq),
        Style::default().fg(color),
    ))
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

fn render_cards(cards: &[Card]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for card in cards {
        match card {
            Card::User { text } => {
                lines.push(Line::from(Span::styled(
                    " you ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                push_wrapped(&mut lines, text, Style::default().fg(Color::Cyan));
                lines.push(Line::from(""));
            }
            Card::Thinking { text, folded } => {
                let n = text.lines().count();
                let title = if *folded {
                    format!(" thinking · {n} lines · Alt+. ")
                } else {
                    " thinking ".into()
                };
                lines.push(Line::from(Span::styled(
                    title,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
                if *folded {
                    if let Some(first) = text.lines().find(|l| !l.trim().is_empty()) {
                        lines.push(Line::from(Span::styled(
                            format!("  {first}…"),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                } else {
                    push_wrapped(&mut lines, text, Style::default().fg(Color::Yellow));
                }
                lines.push(Line::from(""));
            }
            Card::Strike {
                code,
                stdout,
                stderr,
                error,
                ok,
                folded,
            } => {
                let tag = if *ok { " strike " } else { " strike failed " };
                let color = if *ok { Color::Green } else { Color::Red };
                lines.push(Line::from(Span::styled(
                    tag,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )));
                if *folded {
                    let first = code.lines().next().unwrap_or("");
                    let extra = code.lines().count().saturating_sub(1);
                    lines.push(Line::from(Span::styled(
                        format!("  {first}  +{extra}"),
                        Style::default().fg(Color::DarkGray),
                    )));
                } else {
                    push_wrapped(&mut lines, code, Style::default().fg(Color::Green));
                }
                if !stdout.is_empty() {
                    push_wrapped(&mut lines, stdout, Style::default());
                }
                if !stderr.is_empty() {
                    push_wrapped(&mut lines, stderr, Style::default().fg(Color::DarkGray));
                }
                if let Some(err) = error {
                    push_wrapped(&mut lines, err, Style::default().fg(Color::Red));
                }
                lines.push(Line::from(""));
            }
            Card::Answer { text } => {
                lines.push(Line::from(Span::styled(
                    " answer ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )));
                push_wrapped(&mut lines, text, Style::default());
                lines.push(Line::from(""));
            }
            Card::Status { text } => {
                push_wrapped(&mut lines, text, Style::default().fg(Color::DarkGray));
                lines.push(Line::from(""));
            }
        }
    }
    lines
}

fn push_wrapped(lines: &mut Vec<Line<'static>>, text: &str, style: Style) {
    for line in text.trim_end_matches('\n').lines() {
        lines.push(Line::from(Span::styled(line.to_string(), style)));
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
            },
        };
        let s: String = trajectory_line(&ev)
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(s.contains('v'), "{s}");
        assert!(s.contains("strike"), "{s}");
        assert!(s.contains("22ms"), "{s}");
        assert!(s.contains("2+2"), "{s}");
    }
}
