//! smith TUI: transcript blocks, ask worker, `@` file picker.

mod picker;

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
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
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use ratatui::Terminal;

use crate::ask::{self, AskSink, HttpCompleter};
use crate::config::{Config, Provider};
use crate::{default_hammer, default_store, Anvil, StrikeReply};

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
    Ask(String),
    Strike(String),
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
    pub store: PathBuf,
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
}

struct PickerState {
    query: String,
    hits: Vec<FileHit>,
    selected: usize,
}

impl App {
    fn submit_ask(&mut self) {
        let text = self.input.trim_end().to_string();
        if text.is_empty() || self.busy {
            return;
        }
        self.input.clear();
        self.cursor = 0;
        self.picker = None;
        self.cards.push(Card::User { text: text.clone() });
        self.busy = true;
        self.status = "thinking".into();
        self.stick_bottom = true;
        let _ = self.jobs.send(Job::Ask(text));
    }

    fn submit_strike(&mut self) {
        let text = self.input.trim_end().to_string();
        if text.is_empty() || self.busy {
            return;
        }
        self.input.clear();
        self.cursor = 0;
        self.picker = None;
        self.cards.push(Card::User {
            text: format!("(strike)\n{text}"),
        });
        self.busy = true;
        self.status = "striking".into();
        self.stick_bottom = true;
        let _ = self.jobs.send(Job::Strike(text));
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
                    self.cards.push(Card::Thinking { text, folded: true });
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
                self.cards.push(Card::Strike {
                    code,
                    stdout,
                    stderr,
                    error,
                    ok,
                    folded,
                });
            }
            Ev::Answer(text) => {
                if !text.is_empty() {
                    self.cards.push(Card::Answer { text });
                }
                self.busy = false;
                self.status = "idle".into();
                self.stick_bottom = true;
            }
            Ev::Failed(text) => {
                self.cards.push(Card::Status { text });
                self.busy = false;
                self.status = "error".into();
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
    let store = launch.store.clone();
    let hammer = launch.hammer.clone();
    let worker_provider = provider.clone();
    let worker_model = model.clone();
    thread::Builder::new()
        .name("smith-anvil".into())
        .spawn(move || worker(store, hammer, worker_provider, worker_model, jobs_rx, ev_tx))
        .map_err(io::Error::other)?;

    let files = list_files(&launch.cwd, 4000);
    let mut app = App {
        cards: vec![Card::Status {
            text: format!(
                "smith · {} · {} · config {} · @ file  Enter ask  Ctrl+S strike  Ctrl+J newline  Alt+. fold  Ctrl+C quit",
                provider_name,
                model,
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
        provider_name,
        model,
    };

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

fn worker(
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
            Job::Ask(prompt) => {
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
            Job::Strike(code) => {
                let _ = ev.send(Ev::Status("striking".into()));
                match anvil.strike(&code) {
                    Ok(reply) => {
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
                    Err(err) => {
                        let _ = ev.send(Ev::Failed(err.to_string()));
                    }
                }
            }
        }
    }
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    let mut last_tick = Instant::now();
    loop {
        while let Ok(ev) = app.events.try_recv() {
            app.apply(ev);
        }
        if last_tick.elapsed() >= Duration::from_millis(120) {
            app.tick = app.tick.wrapping_add(1);
            last_tick = Instant::now();
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

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
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

    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
        (KeyCode::Esc, _) => {}
        (KeyCode::Enter, KeyModifiers::NONE) => app.submit_ask(),
        (KeyCode::Char('s'), KeyModifiers::CONTROL) => app.submit_strike(),
        (KeyCode::Char('j'), KeyModifiers::CONTROL) | (KeyCode::Enter, KeyModifiers::SHIFT) => {
            app.insert('\n');
        }
        (KeyCode::Char('.'), KeyModifiers::ALT) => app.toggle_last_fold(),
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

fn draw(frame: &mut Frame, app: &App) {
    let picker_h = app
        .picker
        .as_ref()
        .map(|p| (p.hits.len() as u16 + 2).clamp(3, 10))
        .unwrap_or(0);
    let input_h = (app.input.matches('\n').count() as u16 + 1).clamp(1, 8) + 2;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),
            Constraint::Length(picker_h),
            Constraint::Length(input_h),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let lines = render_cards(&app.cards);
    let view_h = chunks[0].height.saturating_sub(2);
    let max_scroll = lines.len().saturating_sub(view_h as usize) as u16;
    let scroll = if app.stick_bottom {
        max_scroll
    } else {
        app.scroll.min(max_scroll)
    };
    let transcript = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" smith "))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(transcript, chunks[0]);

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
        frame.render_widget(Clear, chunks[1]);
        frame.render_widget(list, chunks[1]);
    }

    let compose = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(" ask "));
    frame.render_widget(compose, chunks[2]);
    let (cx, cy) = cursor_in(&app.input, app.cursor);
    frame.set_cursor_position((chunks[2].x + cx + 1, chunks[2].y + cy + 1));

    let spin = if app.busy {
        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"][(app.tick as usize) % 8]
    } else {
        "·"
    };
    frame.render_widget(
        Paragraph::new(Line::from(format!(
            " {spin} {} · {} · {} ",
            app.status, app.provider_name, app.model
        ))),
        chunks[3],
    );
}

fn cursor_in(input: &str, cursor: usize) -> (u16, u16) {
    let before = &input[..cursor.min(input.len())];
    let y = before.matches('\n').count() as u16;
    let x = before.rsplit('\n').next().unwrap_or(before).chars().count() as u16;
    (x, y)
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
        store: default_store(),
        hammer: default_hammer(),
        config: None,
        provider: None,
        model: None,
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}
