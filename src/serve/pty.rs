//! Serve-owned login shells. portable-pty + vt100. Smith only projects.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};

const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const SCROLLBACK: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PtyScreen {
    pub name: String,
    pub cols: u16,
    pub rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub lines: Vec<String>,
    #[serde(default)]
    pub runs: Vec<Vec<PtyRun>>,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PtyRun {
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

fn split_color(c: vt100::Color) -> (Option<u8>, Option<[u8; 3]>) {
    match c {
        vt100::Color::Idx(n) => (Some(n), None),
        vt100::Color::Rgb(r, g, b) => (None, Some([r, g, b])),
        vt100::Color::Default => (None, None),
    }
}

impl PtyRun {
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

impl PtyScreen {
    /// Last two non-empty rows. Inspect chrome; not the full screen.
    pub fn preview(&self) -> Option<String> {
        let mut rows: Vec<&str> = self
            .lines
            .iter()
            .map(|line| line.trim_end())
            .filter(|line| !line.is_empty())
            .rev()
            .take(2)
            .collect();
        if rows.is_empty() {
            return None;
        }
        rows.reverse();
        Some(rows.join(" · "))
    }

    /// Last nonempty rows for the event log / ask.
    pub fn observe_text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.trim_end())
            .filter(|line| !line.is_empty())
            .rev()
            .take(16)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub struct PtyHost {
    map: Mutex<HashMap<String, Arc<LivePty>>>,
    pumps: Mutex<Vec<thread::JoinHandle<()>>>,
}

struct LivePty {
    writer: Mutex<Box<dyn Write + Send>>,
    parser: Mutex<vt100::Parser>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    alive: AtomicBool,
}

impl Default for PtyHost {
    fn default() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            pumps: Mutex::new(Vec::new()),
        }
    }
}

impl PtyHost {
    pub fn names(&self) -> Vec<String> {
        self.map
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn is_hot(&self, name: &str) -> bool {
        self.map
            .lock()
            .ok()
            .and_then(|m| m.get(name).cloned())
            .is_some_and(|p| p.alive())
    }

    pub fn open(&self, name: &str, cols: u16, rows: u16) -> io::Result<PtyScreen> {
        let live = self.ensure(name, cols.max(2), rows.max(2))?;
        live.resize(cols.max(2), rows.max(2))?;
        Ok(live.snap(name))
    }

    pub fn write(&self, name: &str, data: &[u8]) -> io::Result<PtyScreen> {
        let live = self.ensure(name, DEFAULT_COLS, DEFAULT_ROWS)?;
        live.write(data)?;
        Ok(live.snap(name))
    }

    pub fn resize(&self, name: &str, cols: u16, rows: u16) -> io::Result<PtyScreen> {
        let live = self.ensure(name, cols.max(2), rows.max(2))?;
        live.resize(cols.max(2), rows.max(2))?;
        Ok(live.snap(name))
    }

    pub fn snap(&self, name: &str) -> io::Result<PtyScreen> {
        let live = self.ensure(name, DEFAULT_COLS, DEFAULT_ROWS)?;
        Ok(live.snap(name))
    }

    /// Screen if the shell is already up. Does not spawn.
    pub fn peek(&self, name: &str) -> Option<PtyScreen> {
        self.map
            .lock()
            .ok()
            .and_then(|m| m.get(name).cloned())
            .map(|live| live.snap(name))
    }

    pub fn shutdown(&self) {
        let lives: Vec<Arc<LivePty>> = self
            .map
            .lock()
            .map(|mut m| m.drain().map(|(_, live)| live).collect())
            .unwrap_or_default();
        for live in &lives {
            live.terminate();
        }
        let pumps: Vec<thread::JoinHandle<()>> = self
            .pumps
            .lock()
            .map(|mut p| p.drain(..).collect())
            .unwrap_or_default();
        for pump in pumps {
            let _ = pump.join();
        }
    }

    fn ensure(&self, name: &str, cols: u16, rows: u16) -> io::Result<Arc<LivePty>> {
        let mut map = self.map.lock().map_err(|_| io::Error::other("ptys"))?;
        if let Some(live) = map.get(name) {
            if live.reap_if_dead() {
                map.remove(name);
            } else {
                return Ok(live.clone());
            }
        }
        let (live, pump) = spawn(name, cols, rows)?;
        map.insert(name.to_string(), live.clone());
        drop(map);
        if let Ok(mut pumps) = self.pumps.lock() {
            pumps.push(pump);
        }
        Ok(live)
    }
}

fn spawn(name: &str, cols: u16, rows: u16) -> io::Result<(Arc<LivePty>, thread::JoinHandle<()>)> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| io::Error::other(err.to_string()))?;
    let mut cmd = CommandBuilder::new_default_prog();
    cmd.env("TERM", "xterm-256color");
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
    let live = Arc::new(LivePty {
        writer: Mutex::new(writer),
        parser: Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)),
        child: Mutex::new(child),
        master: Mutex::new(pair.master),
        alive: AtomicBool::new(true),
    });
    let pump = live.clone();
    let handle = thread::Builder::new()
        .name(format!("anvil-pty-{name}"))
        .spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(mut parser) = pump.parser.lock() {
                            parser.process(&buf[..n]);
                        }
                    }
                    Err(_) => break,
                }
            }
            pump.alive.store(false, Ordering::Relaxed);
        })?;
    Ok((live, handle))
}

impl LivePty {
    fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    fn reap_if_dead(&self) -> bool {
        if let Ok(mut child) = self.child.lock() {
            if let Ok(Some(_)) = child.try_wait() {
                self.alive.store(false, Ordering::Relaxed);
            }
        }
        !self.alive()
    }

    fn terminate(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.flush();
        }
    }

    fn write(&self, data: &[u8]) -> io::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("pty busy"))?;
        writer.write_all(data)?;
        writer.flush()
    }

    fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.master
            .lock()
            .map_err(|_| io::Error::other("pty busy"))?
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

    fn snap(&self, name: &str) -> PtyScreen {
        let _ = self.reap_if_dead();
        let (lines, runs, cols, rows, cursor_col, cursor_row) = self
            .parser
            .lock()
            .ok()
            .map(|p| screen_lines(p.screen()))
            .unwrap_or_else(|| (Vec::new(), Vec::new(), DEFAULT_COLS, DEFAULT_ROWS, 0, 0));
        PtyScreen {
            name: name.into(),
            cols,
            rows,
            cursor_col,
            cursor_row,
            lines,
            runs,
            alive: self.alive(),
        }
    }
}

impl Drop for LivePty {
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

fn screen_lines(screen: &vt100::Screen) -> (Vec<String>, Vec<Vec<PtyRun>>, u16, u16, u16, u16) {
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let mut lines = Vec::with_capacity(rows as usize);
    let mut all_runs = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut line = String::new();
        let mut runs: Vec<PtyRun> = Vec::new();
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
                let run = PtyRun::from_cell(contents.clone(), cell);
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
                    if last.same_style(&PtyRun::blank()) {
                        last.text.push(' ');
                    } else {
                        runs.push(PtyRun::blank());
                    }
                } else {
                    runs.push(PtyRun::blank());
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
    fn preview_is_the_last_nonempty_row() {
        let screen = PtyScreen {
            name: "bash".into(),
            cols: 8,
            rows: 3,
            cursor_col: 0,
            cursor_row: 1,
            lines: vec!["$ echo hi".into(), "hi".into(), "        ".into()],
            runs: Vec::new(),
            alive: true,
        };
        assert_eq!(screen.preview().as_deref(), Some("$ echo hi · hi"));
    }

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
    fn shutdown_joins_the_pump() {
        let host = PtyHost::default();
        let opened = host.open("bash", 24, 80).unwrap();
        assert!(opened.alive, "{opened:?}");
        let start = Instant::now();
        host.shutdown();
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "pty shutdown hung: {:?}",
            start.elapsed()
        );
        assert!(!host.is_hot("bash"));
    }
}
