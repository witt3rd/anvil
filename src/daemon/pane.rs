//! A pane: the PTY and the character grid.
//! Kernel: "pane — Rectangle. Views a process. Holds the PTY and the
//! character grid." The process runs on the slave; the daemon holds
//! the master and parses its bytes into the grid.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// The process is on the alternate screen (`less`, vim). Wheel
    /// there is keys; on the primary screen it is this view's history.
    #[serde(default, skip_serializing_if = "is_false")]
    pub alternate: bool,
    /// Rows back from the live screen. Zero is the bottom.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub scroll: u16,
    /// Bumps when the pane's view changes. The client sends it back
    /// so an unchanged pane sends no cells.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub gen: u64,
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

fn is_zero_u64(v: &u64) -> bool {
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

const GRID_PACK_VERSION: u8 = 1;

impl Grid {
    /// Packed cells for the mux socket. JSON is the control plane;
    /// this is the frame.
    pub fn pack(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(64 + self.runs.len() * 16);
        o.push(GRID_PACK_VERSION);
        o.extend(self.cols.to_le_bytes());
        o.extend(self.rows.to_le_bytes());
        o.extend(self.cursor_col.to_le_bytes());
        o.extend(self.cursor_row.to_le_bytes());
        let mut flags = 0u8;
        if self.alive {
            flags |= 1;
        }
        if self.acp {
            flags |= 2;
        }
        if self.mouse {
            flags |= 4;
        }
        if self.modify {
            flags |= 8;
        }
        if self.alternate {
            flags |= 16;
        }
        o.push(flags);
        o.extend(self.kitty.to_le_bytes());
        o.extend(self.scroll.to_le_bytes());
        let nrows = self.runs.len().min(u16::MAX as usize) as u16;
        o.extend(nrows.to_le_bytes());
        for row in self.runs.iter().take(nrows as usize) {
            let n = row.len().min(u16::MAX as usize) as u16;
            o.extend(n.to_le_bytes());
            for run in row.iter().take(n as usize) {
                let mut f = 0u8;
                if run.bold {
                    f |= 1;
                }
                if run.italic {
                    f |= 2;
                }
                if run.underline {
                    f |= 4;
                }
                if run.inverse {
                    f |= 8;
                }
                if run.fg.is_some() {
                    f |= 16;
                }
                if run.fg_rgb.is_some() {
                    f |= 32;
                }
                if run.bg.is_some() {
                    f |= 64;
                }
                if run.bg_rgb.is_some() {
                    f |= 128;
                }
                o.push(f);
                if let Some(v) = run.fg {
                    o.push(v);
                }
                if let Some([r, g, b]) = run.fg_rgb {
                    o.extend([r, g, b]);
                }
                if let Some(v) = run.bg {
                    o.push(v);
                }
                if let Some([r, g, b]) = run.bg_rgb {
                    o.extend([r, g, b]);
                }
                let bytes = run.text.as_bytes();
                let len = bytes.len().min(u16::MAX as usize) as u16;
                o.extend(len.to_le_bytes());
                o.extend(&bytes[..len as usize]);
            }
        }
        o
    }

    pub fn unpack(buf: &[u8]) -> io::Result<Grid> {
        let mut i = 0usize;
        let take = |i: &mut usize, n: usize| -> io::Result<&[u8]> {
            let end = i
                .checked_add(n)
                .filter(|end| *end <= buf.len())
                .ok_or_else(|| io::Error::other("truncated grid"))?;
            let slice = &buf[*i..end];
            *i = end;
            Ok(slice)
        };
        let u8_ = |i: &mut usize| -> io::Result<u8> { Ok(take(i, 1)?[0]) };
        let u16_ = |i: &mut usize| -> io::Result<u16> {
            let b = take(i, 2)?;
            Ok(u16::from_le_bytes([b[0], b[1]]))
        };
        if u8_(&mut i)? != GRID_PACK_VERSION {
            return Err(io::Error::other("unknown grid pack"));
        }
        let cols = u16_(&mut i)?;
        let rows = u16_(&mut i)?;
        let cursor_col = u16_(&mut i)?;
        let cursor_row = u16_(&mut i)?;
        let flags = u8_(&mut i)?;
        let kitty = u16_(&mut i)?;
        let scroll = u16_(&mut i)?;
        let nrows = u16_(&mut i)? as usize;
        let mut runs = Vec::with_capacity(nrows);
        let mut lines = Vec::with_capacity(nrows);
        for _ in 0..nrows {
            let n = u16_(&mut i)? as usize;
            let mut row = Vec::with_capacity(n);
            let mut line = String::new();
            for _ in 0..n {
                let f = u8_(&mut i)?;
                let fg = if f & 16 != 0 {
                    Some(u8_(&mut i)?)
                } else {
                    None
                };
                let fg_rgb = if f & 32 != 0 {
                    let b = take(&mut i, 3)?;
                    Some([b[0], b[1], b[2]])
                } else {
                    None
                };
                let bg = if f & 64 != 0 {
                    Some(u8_(&mut i)?)
                } else {
                    None
                };
                let bg_rgb = if f & 128 != 0 {
                    let b = take(&mut i, 3)?;
                    Some([b[0], b[1], b[2]])
                } else {
                    None
                };
                let len = u16_(&mut i)? as usize;
                let text = std::str::from_utf8(take(&mut i, len)?)
                    .map_err(|err| io::Error::other(err))?
                    .to_string();
                line.push_str(&text);
                row.push(Run {
                    text,
                    fg,
                    fg_rgb,
                    bg,
                    bg_rgb,
                    bold: f & 1 != 0,
                    italic: f & 2 != 0,
                    underline: f & 4 != 0,
                    inverse: f & 8 != 0,
                });
            }
            lines.push(line);
            runs.push(row);
        }
        Ok(Grid {
            cols,
            rows,
            cursor_col,
            cursor_row,
            lines,
            runs,
            alive: flags & 1 != 0,
            acp: flags & 2 != 0,
            mouse: flags & 4 != 0,
            kitty,
            modify: flags & 8 != 0,
            alternate: flags & 16 != 0,
            scroll,
            gen: 0,
        })
    }
}

pub struct Pane {
    writer: Mutex<Box<dyn Write + Send>>,
    parser: Mutex<vt100::Parser>,
    keys: Mutex<super::keys::Mode>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    alive: AtomicBool,
    gen: AtomicU64,
    wake: Option<Arc<super::wake::Wake>>,
}

impl Pane {
    /// Spawn a process on the pane's slave PTY. The daemon holds the master.
    pub fn spawn(
        program: &str,
        cols: u16,
        rows: u16,
        cwd: Option<&str>,
        isig: bool,
    ) -> io::Result<Arc<Pane>> {
        Self::spawn_wake(program, cols, rows, cwd, isig, None)
    }

    pub fn spawn_wake(
        program: &str,
        cols: u16,
        rows: u16,
        cwd: Option<&str>,
        isig: bool,
        wake: Option<Arc<super::wake::Wake>>,
    ) -> io::Result<Arc<Pane>> {
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
        if let Some(dir) = cwd.filter(|s| !s.is_empty()) {
            cmd.cwd(dir);
        }
        if !isig {
            if let Some(fd) = pair.master.as_raw_fd() {
                disable_isig(fd);
            }
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
            gen: AtomicU64::new(1),
            wake,
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
                            pump.gen.fetch_add(1, Ordering::Relaxed);
                            if let Some(w) = &pump.wake {
                                w.ping();
                            }
                        }
                        Err(_) => break,
                    }
                }
                pump.alive.store(false, Ordering::Relaxed);
                pump.gen.fetch_add(1, Ordering::Relaxed);
                if let Some(w) = &pump.wake {
                    w.ping();
                }
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
        self.gen.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn gen(&self) -> u64 {
        self.gen.load(Ordering::Relaxed)
    }

    /// Read the pane's grid: its cols, rows, and cells.
    pub fn grid(&self) -> Grid {
        self.grid_at(0)
    }

    /// `scroll` is rows of history above the live screen.
    pub fn grid_at(&self, scroll: usize) -> Grid {
        let _ = self.reap_if_dead();
        let (lines, runs, cols, rows, cursor_col, cursor_row, mouse, alternate, scroll) = self
            .parser
            .lock()
            .ok()
            .map(|mut p| {
                p.set_scrollback(scroll);
                let screen = p.screen();
                let (lines, runs, cols, rows, cursor_col, cursor_row) = screen_lines(screen);
                let mouse = screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None;
                let alternate = screen.alternate_screen();
                let scroll = screen.scrollback() as u16;
                (
                    lines, runs, cols, rows, cursor_col, cursor_row, mouse, alternate, scroll,
                )
            })
            .unwrap_or_else(|| (Vec::new(), Vec::new(), 0, 0, 0, 0, false, false, 0));
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
            alternate,
            scroll,
            gen: self.gen(),
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
                if self.alive.swap(false, Ordering::Relaxed) {
                    self.gen.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        !self.alive()
    }
}

/// Agent TUIs want Ctrl-C as a key. ISIG on the PTY turns `^C` into
/// SIGINT and the process dies — then the pane closes. Shells keep
/// the default so Ctrl-C still interrupts a foreground job.
fn disable_isig(fd: i32) {
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return;
        }
        t.c_lflag &= !libc::ISIG;
        let _ = libc::tcsetattr(fd, libc::TCSANOW, &t);
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
    fn scrollback_shows_older_rows() {
        let mut parser = vt100::Parser::new(4, 20, 50);
        for i in 0..10 {
            parser.process(format!("row{i}\r\n").as_bytes());
        }
        parser.set_scrollback(0);
        let (live, ..) = screen_lines(parser.screen());
        parser.set_scrollback(3);
        let (back, ..) = screen_lines(parser.screen());
        assert_eq!(parser.screen().scrollback(), 3);
        assert_ne!(live[0], back[0], "live={live:?} back={back:?}");
    }

    #[test]
    fn spawn_writes_and_reads_the_grid() {
        let pane = Pane::spawn("sh", 24, 80, None, true).unwrap();
        pane.write(b"printf 'hi from pane'\n").unwrap();
        let grid = wait_for(&pane, |g| {
            g.lines.iter().any(|l| l.contains("hi from pane"))
        });
        assert!(grid.alive, "process should be alive");
    }

    #[test]
    fn exit_marks_the_pane_dead() {
        let pane = Pane::spawn("sh", 24, 80, None, true).unwrap();
        pane.write(b"exit 0\n").unwrap();
        let grid = wait_for(&pane, |g| !g.alive);
        assert!(!grid.alive, "process ended: {grid:?}");
    }

    #[test]
    fn resize_reaches_the_process() {
        let pane = Pane::spawn("sh", 24, 80, None, true).unwrap();
        pane.write(b"trap 'echo got-28x100' WINCH; while :; do sleep 1; done\n")
            .unwrap();
        pane.resize(100, 28).unwrap();
        let grid = wait_for(&pane, |g| g.lines.iter().any(|l| l.contains("got-28x100")));
        assert_eq!(grid.cols, 100);
        assert_eq!(grid.rows, 28);
        pane.terminate();
    }

    #[test]
    fn mouse_follows_the_childs_decset() {
        let pane = Pane::spawn("sh", 24, 80, None, true).unwrap();
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
        let pane = Pane::spawn("sh", 24, 80, None, true).unwrap();
        pane.write(b"printf '\\033[>1u'\n").unwrap();
        let on = wait_for(&pane, |g| g.kitty > 0);
        assert_eq!(on.kitty, 1, "CSI > 1 u should arm kitty keys: {on:?}");
        pane.terminate();
    }

    #[test]
    fn terminate_ends_the_process() {
        let pane = Pane::spawn("sh", 24, 80, None, true).unwrap();
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

    #[test]
    fn packed_grid_round_trips_rgb_runs() {
        let grid = Grid {
            cols: 8,
            rows: 1,
            cursor_col: 5,
            cursor_row: 0,
            lines: vec!["hello   ".into()],
            runs: vec![vec![
                Run {
                    text: "hello".into(),
                    fg: None,
                    fg_rgb: Some([137, 180, 250]),
                    bg: None,
                    bg_rgb: Some([30, 30, 46]),
                    bold: true,
                    italic: false,
                    underline: false,
                    inverse: false,
                },
                Run {
                    text: "   ".into(),
                    fg: Some(7),
                    fg_rgb: None,
                    bg: None,
                    bg_rgb: None,
                    bold: false,
                    italic: false,
                    underline: false,
                    inverse: false,
                },
            ]],
            alive: true,
            acp: false,
            mouse: true,
            kitty: 1,
            modify: false,
            alternate: true,
            scroll: 3,
            gen: 0,
        };
        let packed = grid.pack();
        let json = serde_json::to_vec(&grid).unwrap();
        assert!(
            packed.len() < json.len() / 2,
            "pack {} json {}",
            packed.len(),
            json.len()
        );
        let back = Grid::unpack(&packed).unwrap();
        assert_eq!(grid, back);
    }
}
