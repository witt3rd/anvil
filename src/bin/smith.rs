//! smith — the TUI. Stands at the anvil and swings the hammer.

use std::io;
use std::path::PathBuf;

use anvil::{default_hammer, default_store, Anvil, StrikeReply};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;

#[derive(Parser)]
#[command(
    name = "smith",
    about = "TUI: type Python, anvil strikes, hammer runs it"
)]
struct Cli {
    #[arg(long, env = "ANVIL_STORE")]
    store: Option<PathBuf>,
    #[arg(long, env = "ANVIL_HAMMER")]
    hammer: Option<PathBuf>,
}

struct App {
    anvil: Anvil,
    input: String,
    cursor: usize,
    transcript: Vec<Line<'static>>,
    status: String,
}

impl App {
    fn new(anvil: Anvil) -> Self {
        let store = anvil.store().display().to_string();
        Self {
            anvil,
            input: String::new(),
            cursor: 0,
            transcript: vec![
                Line::from(
                    "smith at the anvil. Type Python. Enter strikes. Ctrl+J newline. Ctrl+C quits.",
                ),
                Line::from(format!("store {store}")),
            ],
            status: "ready".into(),
        }
    }

    fn strike(&mut self) {
        let code = self.input.trim_end().to_string();
        if code.is_empty() {
            return;
        }
        self.input.clear();
        self.cursor = 0;
        self.transcript.push(Line::from(vec![
            Span::styled(">>> ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(code.replace('\n', "\n... ")),
        ]));
        match self.anvil.strike(&code) {
            Ok(reply) => self.push_reply(reply),
            Err(err) => {
                self.status = format!("hammer error: {err}");
                self.transcript
                    .push(Line::from(Span::styled(err.to_string(), Style::default())));
            }
        }
    }

    fn push_reply(&mut self, reply: StrikeReply) {
        if !reply.stdout.is_empty() {
            for line in reply.stdout.trim_end_matches('\n').lines() {
                self.transcript.push(Line::from(line.to_string()));
            }
        }
        if !reply.stderr.is_empty() {
            for line in reply.stderr.trim_end_matches('\n').lines() {
                self.transcript
                    .push(Line::from(Span::raw(line.to_string())));
            }
        }
        if let Some(err) = reply.error.as_deref() {
            for line in err.trim_end_matches('\n').lines() {
                self.transcript.push(Line::from(line.to_string()));
            }
            self.status = "error".into();
        } else if !reply.value.is_null() {
            self.transcript.push(Line::from(reply.value.to_string()));
            self.status = "ok".into();
        } else {
            self.status = "ok".into();
        }
    }

    fn insert(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
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
    }

    fn newline(&mut self) {
        self.insert('\n');
    }
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let store = cli.store.unwrap_or_else(default_store);
    let hammer = cli.hammer.unwrap_or_else(default_hammer);
    let anvil = Anvil::open(&store, &hammer).map_err(|err| io::Error::other(err.to_string()))?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(anvil);
    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(input_height(app) + 2),
                    Constraint::Length(1),
                ])
                .split(frame.area());

            let transcript = Paragraph::new(app.transcript.clone())
                .block(Block::default().borders(Borders::ALL).title(" smith "))
                .wrap(Wrap { trim: false })
                .scroll(scroll_from_bottom(
                    app.transcript.len() as u16,
                    chunks[0].height.saturating_sub(2),
                ));
            frame.render_widget(transcript, chunks[0]);

            let input = Paragraph::new(app.input.as_str())
                .block(Block::default().borders(Borders::ALL).title(" strike "));
            frame.render_widget(input, chunks[1]);

            let cursor_x = {
                let before = &app.input[..app.cursor];
                let last = before.rsplit('\n').next().unwrap_or(before);
                last.chars().count() as u16 + 1
            };
            let cursor_y = app.input[..app.cursor].matches('\n').count() as u16 + 1;
            frame.set_cursor_position((chunks[1].x + cursor_x, chunks[1].y + cursor_y));

            let alive = if app.anvil.hammer_alive() {
                "hammer up"
            } else {
                "hammer down"
            };
            let status = Line::from(format!(
                " {alive} · {} · {} ",
                app.status,
                app.anvil.store().display()
            ));
            frame.render_widget(Paragraph::new(status), chunks[2]);
        })?;

        if event::poll(std::time::Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if handle_key(app, key) {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
        (KeyCode::Enter, KeyModifiers::NONE) => app.strike(),
        (KeyCode::Char('j'), KeyModifiers::CONTROL) | (KeyCode::Enter, KeyModifiers::SHIFT) => {
            app.newline();
        }
        (KeyCode::Char(ch), KeyModifiers::NONE | KeyModifiers::SHIFT) => app.insert(ch),
        (KeyCode::Backspace, _) => app.backspace(),
        _ => {}
    }
    false
}

fn input_height(app: &App) -> u16 {
    let lines = app.input.matches('\n').count() + 1;
    (lines as u16).clamp(1, 8)
}

fn scroll_from_bottom(lines: u16, view: u16) -> (u16, u16) {
    let y = lines.saturating_sub(view);
    (y, 0)
}
