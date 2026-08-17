//! Serve-owned scratch buffers. One file per member: edits/<id>.txt.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::frame::FrameRoot;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditOp {
    Insert,
    Backspace,
    Delete,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Set,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditBuf {
    pub name: String,
    pub text: String,
    pub cursor: usize,
}

impl EditBuf {
    pub fn observe_text(&self) -> String {
        self.text.clone()
    }

    pub fn cursor_row_col(&self) -> (u16, u16) {
        let cur = clamp(&self.text, self.cursor);
        let before = &self.text[..cur];
        let row = before.matches('\n').count() as u16;
        let col = before.rsplit('\n').next().unwrap_or(before).chars().count() as u16;
        (row, col)
    }
}

pub struct EditHost {
    root: PathBuf,
    map: Mutex<HashMap<String, Live>>,
}

struct Live {
    text: String,
    cursor: usize,
}

impl EditHost {
    pub fn new(root: &FrameRoot) -> Self {
        Self {
            root: root.root().to_path_buf(),
            map: Mutex::new(HashMap::new()),
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.map
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn is_hot(&self, name: &str) -> bool {
        self.map.lock().ok().is_some_and(|m| m.contains_key(name))
    }

    pub fn snap(&self, name: &str) -> io::Result<EditBuf> {
        let live = self.ensure(name)?;
        Ok(EditBuf {
            name: name.into(),
            text: live.text,
            cursor: live.cursor,
        })
    }

    pub fn apply(&self, name: &str, op: EditOp, text: &str) -> io::Result<EditBuf> {
        let mut map = self.map.lock().map_err(|_| io::Error::other("edits"))?;
        if !map.contains_key(name) {
            drop(map);
            self.ensure(name)?;
            map = self.map.lock().map_err(|_| io::Error::other("edits"))?;
        }
        let live = map.get_mut(name).ok_or_else(|| io::Error::other("edit"))?;
        live.cursor = clamp(&live.text, live.cursor);
        match op {
            EditOp::Insert => insert(live, text),
            EditOp::Enter => insert(live, "\n"),
            EditOp::Backspace => backspace(live),
            EditOp::Delete => delete_fwd(live),
            EditOp::Left => live.cursor = prev_boundary(&live.text, live.cursor),
            EditOp::Right => live.cursor = next_boundary(&live.text, live.cursor),
            EditOp::Home => live.cursor = line_start(&live.text, live.cursor),
            EditOp::End => live.cursor = line_end(&live.text, live.cursor),
            EditOp::Up => live.cursor = move_vert(&live.text, live.cursor, -1),
            EditOp::Down => live.cursor = move_vert(&live.text, live.cursor, 1),
            EditOp::Set => {
                live.text = text.to_string();
                live.cursor = live.text.len();
            }
        }
        persist(&self.root, name, &live.text)?;
        Ok(EditBuf {
            name: name.into(),
            text: live.text.clone(),
            cursor: live.cursor,
        })
    }

    fn ensure(&self, name: &str) -> io::Result<Live> {
        let mut map = self.map.lock().map_err(|_| io::Error::other("edits"))?;
        if let Some(live) = map.get(name) {
            return Ok(Live {
                text: live.text.clone(),
                cursor: live.cursor,
            });
        }
        let path = edit_path(&self.root, name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = if path.is_file() {
            fs::read_to_string(&path)?
        } else {
            String::new()
        };
        let live = Live { text, cursor: 0 };
        map.insert(
            name.to_string(),
            Live {
                text: live.text.clone(),
                cursor: 0,
            },
        );
        Ok(live)
    }
}

fn edit_path(root: &std::path::Path, name: &str) -> PathBuf {
    root.join("edits").join(format!("{name}.txt"))
}

fn persist(root: &std::path::Path, name: &str, text: &str) -> io::Result<()> {
    let path = edit_path(root, name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)
}

fn clamp(text: &str, cur: usize) -> usize {
    if cur >= text.len() {
        return text.len();
    }
    if text.is_char_boundary(cur) {
        cur
    } else {
        prev_boundary(text, cur)
    }
}

fn prev_boundary(text: &str, cur: usize) -> usize {
    let cur = cur.min(text.len());
    if cur == 0 {
        return 0;
    }
    let mut i = cur - 1;
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_boundary(text: &str, cur: usize) -> usize {
    let cur = cur.min(text.len());
    if cur >= text.len() {
        return text.len();
    }
    let ch = text[cur..]
        .chars()
        .next()
        .map(|c| c.len_utf8())
        .unwrap_or(1);
    cur + ch
}

fn insert(live: &mut Live, s: &str) {
    let cur = clamp(&live.text, live.cursor);
    live.text.insert_str(cur, s);
    live.cursor = cur + s.len();
}

fn backspace(live: &mut Live) {
    let cur = clamp(&live.text, live.cursor);
    if cur == 0 {
        return;
    }
    let prev = prev_boundary(&live.text, cur);
    live.text.replace_range(prev..cur, "");
    live.cursor = prev;
}

fn delete_fwd(live: &mut Live) {
    let cur = clamp(&live.text, live.cursor);
    if cur >= live.text.len() {
        return;
    }
    let next = next_boundary(&live.text, cur);
    live.text.replace_range(cur..next, "");
}

fn line_start(text: &str, cur: usize) -> usize {
    let cur = clamp(text, cur);
    text[..cur].rfind('\n').map(|i| i + 1).unwrap_or(0)
}

fn line_end(text: &str, cur: usize) -> usize {
    let cur = clamp(text, cur);
    match text[cur..].find('\n') {
        Some(i) => cur + i,
        None => text.len(),
    }
}

fn col_chars(text: &str, cur: usize) -> usize {
    let start = line_start(text, cur);
    let cur = clamp(text, cur);
    text[start..cur].chars().count()
}

fn move_vert(text: &str, cur: usize, delta: i32) -> usize {
    let cur = clamp(text, cur);
    let col = col_chars(text, cur);
    let start = line_start(text, cur);
    let target_start = if delta < 0 {
        if start == 0 {
            return cur;
        }
        line_start(text, start - 1)
    } else {
        let end = line_end(text, cur);
        if end >= text.len() {
            return cur;
        }
        end + 1
    };
    let target_end = line_end(text, target_start);
    let mut n = 0;
    let mut i = target_start;
    while i < target_end && n < col {
        i = next_boundary(text, i);
        n += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameRoot;
    use tempfile::TempDir;

    #[test]
    fn insert_enter_backspace_round_trip_on_disk() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let host = EditHost::new(&root);
        host.apply("notes", EditOp::Insert, "hi").unwrap();
        host.apply("notes", EditOp::Enter, "").unwrap();
        host.apply("notes", EditOp::Insert, "there").unwrap();
        let snap = host.snap("notes").unwrap();
        assert_eq!(snap.text, "hi\nthere");
        let path = dir.path().join("edits/notes.txt");
        assert_eq!(fs::read_to_string(path).unwrap(), "hi\nthere");
        host.apply("notes", EditOp::Backspace, "").unwrap();
        assert_eq!(host.snap("notes").unwrap().text, "hi\nther");
    }

    #[test]
    fn up_down_keep_column() {
        let dir = TempDir::new().unwrap();
        let root = FrameRoot::open(dir.path()).unwrap();
        let host = EditHost::new(&root);
        host.apply("n", EditOp::Insert, "abcd\nxy").unwrap();
        host.apply("n", EditOp::Home, "").unwrap();
        host.apply("n", EditOp::Right, "").unwrap();
        host.apply("n", EditOp::Right, "").unwrap();
        host.apply("n", EditOp::Up, "").unwrap();
        let snap = host.snap("n").unwrap();
        assert_eq!(&snap.text[..snap.cursor], "ab");
    }
}
