//! An attached client: the daemon paints its tty. Keys arrive as
//! `input`; pane bytes ping a wake. Chrome is the same draw path as
//! before — the difference is there is no client-side frame clock.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::{
    cursor::{Hide, Show},
    event::EnableFocusChange,
    event::{DisableFocusChange, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::Terminal;

use super::session::Sessions;
use super::wake::Msg;
use crate::tui::Client;

pub fn run(
    fd: RawFd,
    sessions: Arc<Sessions>,
    attached: Option<String>,
    rx: Receiver<Msg>,
) -> io::Result<()> {
    let mut file = unsafe { File::from_raw_fd(fd) };
    let (cols, rows) = tty_size(file.as_raw_fd());
    execute!(
        file,
        EnterAlternateScreen,
        Hide,
        EnableMouseCapture,
        EnableFocusChange
    )?;
    let backend = CrosstermBackend::new(file);
    let mut terminal = Terminal::new(backend)?;
    let mut client = Client::local(sessions, attached);
    client.resize_tty(cols.max(2), rows.max(2))?;
    client.refresh()?;
    terminal.draw(|frame| client.draw(frame))?;
    let mut tick = 0u32;
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(msg) => {
                if apply(&mut client, msg)? {
                    break;
                }
                while let Ok(more) = rx.try_recv() {
                    if apply(&mut client, more)? {
                        break;
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                tick = tick.wrapping_add(1);
                if tick % 2 != 0 && !client.needs_pulse() {
                    continue;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if client.detached {
            break;
        }
        client.bump_tick();
        let _ = client.refresh();
        terminal.draw(|frame| client.draw(frame))?;
    }
    let out = terminal.backend_mut();
    let _ = execute!(
        out,
        DisableMouseCapture,
        DisableFocusChange,
        Show,
        LeaveAlternateScreen
    );
    let _ = out.flush();
    Ok(())
}

fn apply(client: &mut Client, msg: Msg) -> io::Result<bool> {
    match msg {
        Msg::Wake => Ok(false),
        Msg::Input(ev) => {
            client.apply_input(ev)?;
            Ok(client.detached)
        }
    }
}

fn tty_size(fd: RawFd) -> (u16, u16) {
    unsafe {
        let mut w: libc::winsize = std::mem::zeroed();
        if libc::ioctl(fd, libc::TIOCGWINSZ, &mut w) == 0 {
            (w.ws_col.max(1), w.ws_row.max(1))
        } else {
            (80, 24)
        }
    }
}

use std::os::fd::AsRawFd;
