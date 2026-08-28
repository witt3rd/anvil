//! An attached client: the daemon paints its tty. Keys arrive as
//! `input`; pane bytes ping a wake. Chrome is the same draw path as
//! before — the difference is there is no client-side frame clock.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::{
    cursor::{Hide, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};

use super::session::Sessions;
use super::wake::Msg;
use crate::tui::Client;

/// Crossterm's `terminal::size` ioctls `/dev/tty` or the daemon's
/// stdout — neither is the donated screen under systemd. Size the
/// viewport from this fd.
pub fn run(
    fd: RawFd,
    sessions: Arc<Sessions>,
    attached: Option<String>,
    rx: Receiver<Msg>,
) -> io::Result<()> {
    let mut file = unsafe { File::from_raw_fd(fd) };
    let restore_fd = unsafe { libc::dup(file.as_raw_fd()) };
    if restore_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let _restore = Restore(restore_fd);
    let (cols, rows) = tty_size(file.as_raw_fd());
    execute!(file, EnterAlternateScreen, Hide, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(file);
    let area = Rect::new(0, 0, cols.max(2), rows.max(2));
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(area),
        },
    )?;
    let mut client = Client::local(sessions, attached);
    client.resize_tty(area.width, area.height)?;
    client.refresh()?;
    terminal.draw(|frame| client.draw(frame))?;
    let mut tick = 0u32;
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(msg) => {
                if apply(&mut client, &mut terminal, msg)? {
                    break;
                }
                while let Ok(more) = rx.try_recv() {
                    if apply(&mut client, &mut terminal, more)? {
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
    Ok(())
}

fn apply(
    client: &mut Client,
    terminal: &mut Terminal<CrosstermBackend<File>>,
    msg: Msg,
) -> io::Result<bool> {
    match msg {
        Msg::Wake => Ok(false),
        Msg::Input(crate::proto::Input::Resize { cols, rows }) => {
            let cols = cols.max(2);
            let rows = rows.max(2);
            client.resize_tty(cols, rows)?;
            terminal.resize(Rect::new(0, 0, cols, rows))?;
            Ok(false)
        }
        Msg::Input(ev) => {
            client.apply_input(ev)?;
            Ok(client.detached)
        }
    }
}

struct Restore(RawFd);

impl Drop for Restore {
    fn drop(&mut self) {
        let mut file = unsafe { File::from_raw_fd(self.0) };
        let _ = execute!(file, DisableMouseCapture, Show, LeaveAlternateScreen);
        let _ = file.flush();
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
