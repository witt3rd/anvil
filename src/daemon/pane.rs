//! A pane: the PTY and the character grid.
//! Kernel: "pane — Rectangle. Views a process. Holds the PTY and the
//! character grid." The process runs on the slave; the daemon holds
//! the master and parses its bytes into the grid.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};

const SCROLLBACK: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub lines: Vec<String>,
    pub runs: Vec<Vec<Run>>,
    pub alive: bool,
    /// This pane's process speaks ACP. The client does not spawn a shell on it.
    #[serde(default)]
    pub acp: bool,
    /// The process asked for mouse tracking (DECSET 1000/1002/1003).
    /// The client writes SGR mouse only while this is true.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mouse: bool,
    /// Kitty keyboard flags the process asked for (`CSI > flags u`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub kitty: u16,
    /// xterm modifyOtherKeys (`CSI > 4 ; 1/2 m`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub modify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub inverse: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn is_zero(v: &u16) -> bool {
    *v == 0
}

fn split_color(c: vt100::Color) -> (Option<u8>, Option<[u8; 3]>) {
    match c {
        vt100::Color::Idx(n) => (Some(n), None),
        vt100::Color::Rgb(r, g, b) => (None, Some([r, g, b])),
        vt100::Color::Default => (None, None),
    }
}

impl Run {
    fn from_cell(text: String, cell: &vt100::Cell) -> Self {
        let (fg, fg_rgb) = split_color(cell.fgcolor());
        let (bg, bg_rgb) = split_color(cell.bgcolor());
        Self {
            text,
            fg,
            fg_rgb,
            bg,
            bg_rgb,
            bold: cell.bold(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        }
    }

    fn blank() -> Self {
        Self {
            text: " ".into(),
            fg: None,
            fg_rgb: None,
            bg: None,
            bg_rgb: None,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }

    fn same_style(&self, other: &Self) -> bool {
        self.fg == other.fg
            && self.fg_rgb == other.fg_rgb
            && self.bg == other.bg
            && self.bg_rgb == other.bg_rgb
            && self.bold == other.bold
            && self.italic == other.italic
            && self.underline == other.underline
            && self.inverse == other.inverse
    }
}

pub struct Pane {
    writer: Mutex<Box<dyn Write + Send>>,
    parser: Mutex<vt100::Parser>,
    keys: Mutex<super::keys::Mode>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    alive: AtomicBool,
}

impl Pane {
    /// Spawn a process on the pane's slave PTY. The daemon holds the master.
    pub fn spawn(program: &str, cols: u16, rows: u16) -> io::Result<Arc<Pane>> {
        let cols = cols.max(2);
        let rows = rows.max(2);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| io::Error::other(err.to_string()))?;
        let mut parts = program.split_whitespace();
        let bin = parts.next().unwrap_or(program);
        let mut cmd = CommandBuilder::new(bin);
        for arg in parts {
            cmd.arg(arg);
        }
        cmd.env("TERM", "xterm-256color");
        // systemd --user PATH is /usr/bin. Catalog agents live in
        // ~/.local/bin and mise shims so catalog programs resolve.
        if let Ok(home) = std::env::var("HOME") {
            cmd.env("HOME", &home);
            let mut path = format!("{home}/.local/bin:{home}/.local/share/mise/shims");
            if let Ok(p) = std::env::var("PATH") {
                path = format!("{path}:{p}");
            }
            cmd.env("PATH", path);
        }
        if let Ok(shell) = std::env::var("SHELL") {
            cmd.env("SHELL", shell);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|err| io::Error::other(err.to_string()))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| io::Error::other(err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| io::Error::other(err.to_string()))?;
        let pane = Arc::new(Pane {
            writer: Mutex::new(writer),
            parser: Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)),
            keys: Mutex::new(super::keys::Mode::default()),
            child: Mutex::new(child),
            master: Mutex::new(pair.master),
            alive: AtomicBool::new(true),
        });
        let pump = pane.clone();
        thread::Builder::new()
            .name("anvil-pane".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut parser) = pump.parser.lock() {
                                parser.process(&buf[..n]);
                            }
                            if let Ok(mut keys) = pump.keys.lock() {
                                keys.feed(&buf[..n]);
                            }
                        }
                        Err(_) => break,
                    }
                }
                pump.alive.store(false, Ordering::Relaxed);
            })
            .map_err(|err| io::Error::other(err.to_string()))?;
        Ok(pane)
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.lock().ok()?.process_id()
    }

    /// Write to the process. The client's keys go to the focused pane's
    /// process.
    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        if !self.alive() {
            return Err(io::Error::other("the pane's process has ended"));
        }
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("pane busy"))?;
        writer.write_all(data)?;
        writer.flush()
    }

    /// Resize the pane. The process is told (`SIGWINCH`) by the PTY.
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let cols = cols.max(2);
        let rows = rows.max(2);
        self.master
            .lock()
            .map_err(|_| io::Error::other("pane busy"))?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| io::Error::other(err.to_string()))?;
        if let Ok(mut parser) = self.parser.lock() {
            parser.set_size(rows, cols);
        }
        Ok(())
    }

    /// Read the pane's grid: its cols, rows, and cells.
    pub fn grid(&self) -> Grid {
        let _ = self.reap_if_dead();
        let (lines, runs, cols, rows, cursor_col, cursor_row, mouse) = self
            .parser
            .lock()
            .ok()
            .map(|p| {
                let (lines, runs, cols, rows, cursor_col, cursor_row) = screen_lines(p.screen());
                let mouse =
                    p.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None;
                (lines, runs, cols, rows, cursor_col, cursor_row, mouse)
            })
            .unwrap_or_else(|| (Vec::new(), Vec::new(), 0, 0, 0, 0, false));
        let (kitty, modify) = self
            .keys
            .lock()
            .ok()
            .map(|k| (k.kitty(), k.modify()))
            .unwrap_or((0, false));
        Grid {
            cols,
            rows,
            cursor_col,
            cursor_row,
            lines,
            runs,
            alive: self.alive(),
            acp: false,
            mouse,
            kitty,
            modify,
        }
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// End the process. The pump exits when the master's reader EOFs.
    pub fn terminate(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.flush();
        }
    }

    /// Hang up the process: `SIGHUP`, as the kernel's destroy demands.
    pub fn hangup(&self) {
        let pid = self.child.lock().ok().and_then(|c| c.process_id());
        if let Some(pid) = pid {
            unsafe {
                libc::kill(pid as i32, libc::SIGHUP);
            }
        }
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.flush();
        }
    }

    fn reap_if_dead(&self) -> bool {
        if let Ok(mut child) = self.child.lock() {
            if let Ok(Some(_)) = child.try_wait() {
                self.alive.store(false, Ordering::Relaxed);
            }
        }
        !self.alive()
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        self.terminate();
        if let Ok(mut child) = self.child.lock() {
            for _ in 0..20 {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn screen_lines(screen: &vt100::Screen) -> (Vec<String>, Vec<Vec<Run>>, u16, u16, u16, u16) {
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let mut lines = Vec::with_capacity(rows as usize);
    let mut all_runs = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut line = String::new();
        let mut runs: Vec<Run> = Vec::new();
        let mut col = 0u16;
        while col < cols {
            if let Some(cell) = screen.cell(row, col) {
                if cell.is_wide_continuation() {
                    col = col.saturating_add(1);
                    continue;
                }
                let contents = if cell.contents().is_empty() {
                    " ".to_string()
                } else {
                    cell.contents()
                };
                let run = Run::from_cell(contents.clone(), cell);
                line.push_str(&contents);
                if let Some(last) = runs.last_mut() {
                    if last.same_style(&run) {
                        last.text.push_str(&contents);
                    } else {
                        runs.push(run);
                    }
                } else {
                    runs.push(run);
                }
                col = col.saturating_add(if cell.is_wide() { 2 } else { 1 });
            } else {
                line.push(' ');
                if let Some(last) = runs.last_mut() {
                    if last.same_style(&Run::blank()) {
                        last.text.push(' ');
                    } else {
                        runs.push(Run::blank());
                    }
                } else {
                    runs.push(Run::blank());
                }
                col = col.saturating_add(1);
            }
        }
        lines.push(line);
        all_runs.push(runs);
    }
    (lines, all_runs, cols, rows, cursor_col, cursor_row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn rgb_and_bg_survive_the_parser() {
        let mut parser = vt100::Parser::new(2, 40, 0);
        parser.process(b"\x1b[48;2;30;30;46m\x1b[38;2;137;180;250mhello\x1b[0m");
        let (_, runs, ..) = screen_lines(parser.screen());
        let hello = runs[0]
            .iter()
            .find(|r| r.text.contains("hello"))
            .expect("hello run");
        assert_eq!(hello.fg_rgb, Some([137, 180, 250]), "{hello:?}");
        assert_eq!(hello.bg_rgb, Some([30, 30, 46]), "{hello:?}");
    }

    #[test]
    fn spawn_writes_and_reads_the_grid() {
        let pane = Pane::spawn("sh", 24, 80).unwrap();
        pane.write(b"printf 'hi from pane'\n").unwrap();
        let grid = wait_for(&pane, |g| g.lines.iter().any(|l| l.contains("hi from pane")));
        assert!(grid.alive, "process should be alive");
    }

    #[test]
    fn exit_marks_the_pane_dead() {
        let pane = Pane::spawn("sh", 24, 80).unwrap();
        pane.write(b"exit 0\n").unwrap();
        let grid = wait_for(&pane, |g| !g.alive);
        assert!(!grid.alive, "process ended: {grid:?}");
    }

    #[test]
    fn resize_reaches_the_process() {
        let pane = Pane::spawn("sh", 24, 80).unwrap();
        pane.write(b"trap 'echo got-28x100' WINCH; while :; do sleep 1; done\n").unwrap();
        pane.resize(100, 28).unwrap();
        let grid = wait_for(&pane, |g| g.lines.iter().any(|l| l.contains("got-28x100")));
        assert_eq!(grid.cols, 100);
        assert_eq!(grid.rows, 28);
        pane.terminate();
    }

    #[test]
    fn mouse_follows_the_childs_decset() {
        let pane = Pane::spawn("sh", 24, 80).unwrap();
        pane.write(b"printf '\\033[?1000h'\n").unwrap();
        let on = wait_for(&pane, |g| g.mouse);
        assert!(on.mouse, "DECSET 1000 should arm mouse: {on:?}");
        pane.write(b"printf '\\033[?1000l'\n").unwrap();
        let off = wait_for(&pane, |g| !g.mouse);
        assert!(!off.mouse, "DECRST 1000 should clear mouse: {off:?}");
        pane.terminate();
    }

    #[test]
    fn kitty_flags_follow_the_child() {
        let pane = Pane::spawn("sh", 24, 80).unwrap();
        pane.write(b"printf '\\033[>1u'\n").unwrap();
        let on = wait_for(&pane, |g| g.kitty > 0);
        assert_eq!(on.kitty, 1, "CSI > 1 u should arm kitty keys: {on:?}");
        pane.terminate();
    }

    #[test]
    fn terminate_ends_the_process() {
        let pane = Pane::spawn("sh", 24, 80).unwrap();
        assert!(pane.alive());
        pane.terminate();
        let start = Instant::now();
        let grid = wait_for(&pane, |g| !g.alive);
        assert!(!grid.alive);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "pane did not die promptly: {:?}",
            start.elapsed()
        );
    }

    fn wait_for(pane: &Pane, done: impl Fn(&Grid) -> bool) -> Grid {
        let mut grid = pane.grid();
        for _ in 0..100 {
            if done(&grid) {
                return grid;
            }
            std::thread::sleep(Duration::from_millis(20));
            grid = pane.grid();
        }
        grid
    }
}
