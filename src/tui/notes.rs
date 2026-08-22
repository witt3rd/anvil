//! The window's note: a markdown blob the daemon stores with the
//! window. The client edits it in a text box.

use ratatui::layout::Rect;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Open editor for one window's note.
#[derive(Debug, Clone)]
pub struct Notes {
    pub window: String,
    lines: Vec<String>,
    row: usize,
    col: usize,
}

impl Notes {
    pub fn open(window: String, text: &str) -> Notes {
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Notes {
            window,
            lines,
            row: 0,
            col: 0,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn row(&self) -> usize {
        self.row
    }

    pub fn col(&self) -> usize {
        self.col
    }

    pub fn line(&self, i: usize) -> &str {
        self.lines.get(i).map(String::as_str).unwrap_or("")
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The current line is a markdown task (`- [ ]` / `- [x]`).
    pub fn on_task(&self) -> bool {
        task_box(self.line(self.row)).is_some()
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return true;
        }
        match key.code {
            KeyCode::Esc => false,
            KeyCode::Enter => {
                self.split_line();
                true
            }
            KeyCode::Backspace => {
                self.backspace();
                true
            }
            KeyCode::Delete => {
                self.delete();
                true
            }
            KeyCode::Left => {
                self.move_left();
                true
            }
            KeyCode::Right => {
                self.move_right();
                true
            }
            KeyCode::Up => {
                self.move_vert(-1);
                true
            }
            KeyCode::Down => {
                self.move_vert(1);
                true
            }
            KeyCode::Home => {
                self.col = 0;
                true
            }
            KeyCode::End => {
                self.col = chars(self.line(self.row));
                true
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => false,
            KeyCode::Char(' ') if self.toggle_at_cursor() => true,
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() => {
                self.insert(c);
                true
            }
            KeyCode::Tab => {
                self.insert(' ');
                self.insert(' ');
                true
            }
            _ => true,
        }
    }

    pub fn click(&mut self, inner: Rect, col: u16, row: u16, scroll: usize) {
        if row < inner.y || row >= inner.bottom() {
            return;
        }
        let i = (row - inner.y) as usize + scroll;
        if i >= self.lines.len() {
            return;
        }
        self.row = i;
        let x = col.saturating_sub(inner.x) as usize;
        self.col = chars(self.line(i)).min(x);
        let _ = self.toggle_at_cursor();
    }

    fn insert(&mut self, c: char) {
        let line = &mut self.lines[self.row];
        let i = byte_at(line, self.col);
        line.insert(i, c);
        self.col += 1;
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            let line = &mut self.lines[self.row];
            let end = byte_at(line, self.col);
            let start = byte_at(line, self.col - 1);
            line.replace_range(start..end, "");
            self.col -= 1;
            return;
        }
        if self.row == 0 {
            return;
        }
        let rest = self.lines.remove(self.row);
        self.row -= 1;
        self.col = chars(&self.lines[self.row]);
        self.lines[self.row].push_str(&rest);
    }

    fn delete(&mut self) {
        let len = chars(self.line(self.row));
        if self.col < len {
            let line = &mut self.lines[self.row];
            let start = byte_at(line, self.col);
            let end = byte_at(line, self.col + 1);
            line.replace_range(start..end, "");
            return;
        }
        if self.row + 1 >= self.lines.len() {
            return;
        }
        let rest = self.lines.remove(self.row + 1);
        self.lines[self.row].push_str(&rest);
    }

    fn split_line(&mut self) {
        let line = &mut self.lines[self.row];
        let i = byte_at(line, self.col);
        let rest = line.split_off(i);
        self.row += 1;
        self.col = 0;
        self.lines.insert(self.row, rest);
    }

    fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = chars(self.line(self.row));
        }
    }

    fn move_right(&mut self) {
        let len = chars(self.line(self.row));
        if self.col < len {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn move_vert(&mut self, dir: i32) {
        let next = self.row as i32 + dir;
        if next < 0 || next >= self.lines.len() as i32 {
            return;
        }
        self.row = next as usize;
        self.col = self.col.min(chars(self.line(self.row)));
    }

    /// Toggle a markdown task when the cursor sits on `[ ]` / `[x]`.
    fn toggle_at_cursor(&mut self) -> bool {
        let Some((at, checked)) = task_box(self.line(self.row)) else {
            return false;
        };
        if self.col < at || self.col > at + 2 {
            return false;
        }
        let line = &mut self.lines[self.row];
        let start = byte_at(line, at);
        let mark = if checked { "[ ]" } else { "[x]" };
        let end = byte_at(line, at + 3);
        line.replace_range(start..end, mark);
        true
    }
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

fn byte_at(s: &str, col: usize) -> usize {
    s.char_indices()
        .nth(col)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

/// Char index of `[` in a GFM task line, and whether it is checked.
pub fn task_box(line: &str) -> Option<(usize, bool)> {
    let trimmed = line.trim_start();
    let pad = chars(line) - chars(trimmed);
    for (pat, checked) in [
        ("- [x]", true),
        ("- [X]", true),
        ("- [ ]", false),
        ("* [x]", true),
        ("* [X]", true),
        ("* [ ]", false),
    ] {
        if trimmed.starts_with(pat) {
            return Some((pad + 2, checked));
        }
    }
    None
}

pub fn notes_box(area: Rect) -> Rect {
    let width = (area.width * 3 / 4).clamp(36, 72);
    let height = (area.height * 2 / 3).clamp(10, 22);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEventKind;

    fn press(code: KeyCode) -> KeyEvent {
        let mut key = KeyEvent::new(code, KeyModifiers::NONE);
        key.kind = KeyEventKind::Press;
        key
    }

    #[test]
    fn enter_splits_and_esc_yields_the_blob() {
        let mut n = Notes::open("ui".into(), "");
        for c in "one".chars() {
            n.key(press(KeyCode::Char(c)));
        }
        n.key(press(KeyCode::Enter));
        for c in "two".chars() {
            n.key(press(KeyCode::Char(c)));
        }
        assert_eq!(n.text(), "one\ntwo");
        assert!(!n.key(press(KeyCode::Esc)));
    }

    #[test]
    fn space_on_the_box_toggles_a_task() {
        let mut n = Notes::open("ui".into(), "- [ ] write notes");
        n.col = 3;
        assert!(n.on_task());
        n.key(press(KeyCode::Char(' ')));
        assert_eq!(n.text(), "- [x] write notes");
        n.key(press(KeyCode::Char(' ')));
        assert_eq!(n.text(), "- [ ] write notes");
        n.col = 10;
        n.key(press(KeyCode::Char(' ')));
        assert_eq!(n.text(), "- [ ] writ e notes");
    }
}
