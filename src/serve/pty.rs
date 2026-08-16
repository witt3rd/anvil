//! Serve-owned login shells. portable-pty + vt100. Smith only projects.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

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
    pub alive: bool,
}

pub struct PtyHost {
    map: Mutex<HashMap<String, Arc<LivePty>>>,
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

    fn ensure(&self, name: &str, cols: u16, rows: u16) -> io::Result<Arc<LivePty>> {
        let mut map = self.map.lock().map_err(|_| io::Error::other("ptys"))?;
        if let Some(live) = map.get(name) {
            if live.reap_if_dead() {
                map.remove(name);
            } else {
                return Ok(live.clone());
            }
        }
        let live = spawn(name, cols, rows)?;
        map.insert(name.to_string(), live.clone());
        Ok(live)
    }
}

fn spawn(name: &str, cols: u16, rows: u16) -> io::Result<Arc<LivePty>> {
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
    thread::Builder::new()
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
    Ok(live)
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
        let (lines, cols, rows, cursor_col, cursor_row) = self
            .parser
            .lock()
            .ok()
            .map(|p| screen_lines(p.screen()))
            .unwrap_or_else(|| (Vec::new(), DEFAULT_COLS, DEFAULT_ROWS, 0, 0));
        PtyScreen {
            name: name.into(),
            cols,
            rows,
            cursor_col,
            cursor_row,
            lines,
            alive: self.alive(),
        }
    }
}

impl Drop for LivePty {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn screen_lines(screen: &vt100::Screen) -> (Vec<String>, u16, u16, u16, u16) {
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let mut lines = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut line = String::new();
        let mut col = 0u16;
        while col < cols {
            if let Some(cell) = screen.cell(row, col) {
                if cell.is_wide_continuation() {
                    col = col.saturating_add(1);
                    continue;
                }
                let contents = cell.contents();
                if contents.is_empty() {
                    line.push(' ');
                } else {
                    line.push_str(&contents);
                }
                col = col.saturating_add(if cell.is_wide() { 2 } else { 1 });
            } else {
                line.push(' ');
                col = col.saturating_add(1);
            }
        }
        lines.push(line);
    }
    (lines, cols, rows, cursor_col, cursor_row)
}
