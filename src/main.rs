// As a user: run `anvil` to become the client, attaching to a session.
// The session persists on disk (kernel: "Sessions, windows, and panes stay").
// Allocate a window -> pane -> process (e.g., nvim).
// Each pane holds a PTY; the process runs on the slave PTY.
// On reattach, the client repaints from the daemon's character grid.
use std::io;
use std::path::PathBuf;

use anvil::daemon;
use clap::{Parser, Subcommand};
use ratatui::Terminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::layout::{Layout, Constraint};
use ratatui::widgets::{Block, BorderType};
use ratatui::prelude::Stylize;
use opaline::Theme;

#[derive(Parser)]
#[command(name = "anvil")]
#[command(about = "Terminal multiplexer", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// The daemon: owns sessions, serves clients over a unix socket.
    Daemon {
        /// Override the socket path (`ANVIL_SOCK`).
        #[arg(long)]
        sock: Option<PathBuf>,
        /// Override the state root (`~/.anvil` by default).
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Daemon { sock, root }) => {
            let sock = sock.unwrap_or_else(daemon::default_sock);
            let root = root.unwrap_or_else(|| std::env::var("ANVIL_ROOT").unwrap_or_else(|_| default_root()).into());
            daemon::run(root, sock)
        }
        None => {
            // The client: run `anvil` to become the client, attaching to
            // a session. The session persists on disk (kernel: "Sessions,
            // windows, and panes stay").
            let sock = daemon::default_sock();
            let root = std::env::var("ANVIL_ROOT").unwrap_or_else(|_| default_root());
            daemon::ensure_running(&sock, std::path::Path::new(&root))?;

            let mut terminal = ratatui::init();
            let out = run(&mut terminal);
            ratatui::restore();
            out
        }
    }
}

fn default_root() -> String {
    std::env::var("HOME").map(|h| format!("{h}/.anvil")).unwrap_or_else(|_| ".anvil".into())
}

fn run(terminal: &mut Terminal<impl ratatui::backend::Backend>) -> io::Result<()> {
    let theme = Theme::default();
    loop {
        terminal.draw(|frame| draw(frame, &theme)).ok();
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                break;
            }
        }
    }
    Ok(())
}

fn draw(frame: &mut ratatui::Frame, theme: &Theme) {
    let chunks = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(80),
    ])
    .split(frame.area());

    let left_bg: ratatui::style::Color = theme.color("bg.selection").into();
    let right_bg: ratatui::style::Color = theme.color("bg.base").into();

    let left = Block::default()
        .border_type(BorderType::Rounded)
        .bg(left_bg);

    let right = Block::default()
        .border_type(BorderType::Rounded)
        .bg(right_bg);

    frame.render_widget(left, chunks[0]);
    frame.render_widget(right, chunks[1]);
}