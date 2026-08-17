// As a user: run `anvil` to become the client, attaching to a session.
// The session persists on disk (kernel: "Sessions, windows, and panes stay").
// Allocate a window -> pane -> process (e.g., nvim).
// Each pane holds a PTY; the process runs on the slave PTY.
// On reattach, the client repaints from the daemon's character grid.
use std::io;

use clap::Parser;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::layout::{Layout, Constraint};
use ratatui::widgets::{Block, BorderType};
use ratatui::prelude::Stylize;
use opaline::Theme;

#[derive(Parser)]
#[command(name = "anvil")]
#[command(about = "Terminal multiplexer", long_about = None)]
struct Cli;

fn main() -> io::Result<()> {
    let _cli = Cli::parse();

    let mut terminal = ratatui::init();
    let out = run(&mut terminal);
    ratatui::restore();
    out
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