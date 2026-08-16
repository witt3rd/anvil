//! Last-paint cell map + drag-select. Copy-on-select matches herdr.

use std::collections::HashMap;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme::Face;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub col: u16,
    pub row: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub pane: String,
    pub start: Cell,
    pub end: Cell,
    pub finalized: bool,
}

impl Selection {
    pub fn begin(pane: impl Into<String>, col: u16, row: u16) -> Self {
        let cell = Cell { col, row };
        Self {
            pane: pane.into(),
            start: cell,
            end: cell,
            finalized: false,
        }
    }

    pub fn drag(&mut self, col: u16, row: u16) {
        if !self.finalized {
            self.end = Cell { col, row };
        }
    }

    pub fn was_just_click(&self) -> bool {
        self.start == self.end
    }

    pub fn finish(&mut self) {
        self.finalized = true;
    }

    pub fn ordered(&self) -> (Cell, Cell) {
        let a = self.start;
        let b = self.end;
        if (a.row, a.col) <= (b.row, b.col) {
            (a, b)
        } else {
            (b, a)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Painted {
    panes: HashMap<String, PanePaint>,
}

#[derive(Debug, Clone)]
struct PanePaint {
    area: Rect,
    lines: Vec<String>,
}

impl Painted {
    pub fn clear(&mut self) {
        self.panes.clear();
    }

    pub fn record(&mut self, pane: &str, area: Rect, lines: &[Line<'_>]) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.panes.insert(
            pane.to_string(),
            PanePaint {
                area,
                lines: lines.iter().map(line_text).collect(),
            },
        );
    }

    pub fn extract(&self, sel: &Selection) -> Option<String> {
        let paint = self.panes.get(&sel.pane)?;
        let (a, b) = sel.ordered();
        let mut out = Vec::new();
        for row in a.row..=b.row {
            if row < paint.area.y || row >= paint.area.y.saturating_add(paint.area.height) {
                continue;
            }
            let i = (row - paint.area.y) as usize;
            let line = paint.lines.get(i).cloned().unwrap_or_default();
            let start = if row == a.row {
                a.col.saturating_sub(paint.area.x)
            } else {
                0
            };
            let end = if row == b.row {
                b.col.saturating_sub(paint.area.x).saturating_add(1)
            } else {
                line.chars().count() as u16
            };
            out.push(slice_chars(&line, start, end));
        }
        let text = out.join("\n");
        let trimmed = text.trim_end_matches('\n');
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    pub fn word_at(&self, pane: &str, col: u16, row: u16) -> Option<Selection> {
        let paint = self.panes.get(pane)?;
        if row < paint.area.y || col < paint.area.x {
            return None;
        }
        let i = (row - paint.area.y) as usize;
        let line = paint.lines.get(i)?;
        let x = (col - paint.area.x) as usize;
        let (start, end) = word_bounds(line, x)?;
        Some(Selection {
            pane: pane.to_string(),
            start: Cell {
                col: paint.area.x.saturating_add(start as u16),
                row,
            },
            end: Cell {
                col: paint.area.x.saturating_add(end.saturating_sub(1) as u16),
                row,
            },
            finalized: true,
        })
    }
}

pub fn line_text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn apply_highlight(
    lines: &[Line<'static>],
    area: Rect,
    sel: Option<&Selection>,
    pane: &str,
) -> Vec<Line<'static>> {
    let Some(sel) = sel.filter(|s| s.pane == pane) else {
        return lines.to_vec();
    };
    let (a, b) = sel.ordered();
    let style = super::theme::t().style(Face::Select);
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let row = area.y.saturating_add(i as u16);
            if row < a.row || row > b.row {
                return line.clone();
            }
            let start = if row == a.row {
                a.col.saturating_sub(area.x)
            } else {
                0
            };
            let end = if row == b.row {
                b.col.saturating_sub(area.x).saturating_add(1)
            } else {
                u16::MAX
            };
            highlight_line(line, start, end, style)
        })
        .collect()
}

fn highlight_line(line: &Line<'static>, start: u16, end: u16, hi: Style) -> Line<'static> {
    if start >= end {
        return line.clone();
    }
    let mut spans = Vec::new();
    let mut col = 0u16;
    for span in &line.spans {
        for ch in span.content.chars() {
            let next = col.saturating_add(1);
            let style = if col >= start && col < end {
                hi.add_modifier(Modifier::REVERSED)
            } else {
                span.style
            };
            push_char(&mut spans, ch, style);
            col = next;
        }
    }
    Line::from(spans)
}

fn push_char(spans: &mut Vec<Span<'static>>, ch: char, style: Style) {
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push(ch);
            return;
        }
    }
    spans.push(Span::styled(ch.to_string(), style));
}

fn slice_chars(s: &str, start: u16, end: u16) -> String {
    s.chars()
        .skip(start as usize)
        .take(end.saturating_sub(start) as usize)
        .collect()
}

pub fn word_bounds(line: &str, col: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let col = col.min(chars.len().saturating_sub(1));
    if !is_word(chars[col]) {
        return Some((col, col + 1));
    }
    let mut start = col;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col + 1;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    Some((start, end))
}

fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == '/'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(lines: &[&str]) -> Painted {
        let mut painted = Painted::default();
        let area = Rect::new(2, 3, 20, lines.len() as u16);
        let rat: Vec<Line> = lines.iter().map(|s| Line::raw((*s).to_string())).collect();
        painted.record("audit", area, &rat);
        painted
    }

    #[test]
    fn drag_extracts_inclusive_range() {
        let painted = paint(&["alpha beta", "gamma"]);
        let mut sel = Selection::begin("audit", 2, 3);
        sel.drag(6, 3);
        assert_eq!(painted.extract(&sel).as_deref(), Some("alpha"));
        sel.drag(6, 4);
        assert_eq!(painted.extract(&sel).as_deref(), Some("alpha beta\ngamma"));
    }

    #[test]
    fn click_is_one_cell() {
        let sel = Selection::begin("audit", 4, 3);
        assert!(sel.was_just_click());
        let painted = paint(&["alpha"]);
        assert_eq!(painted.extract(&sel).as_deref(), Some("p"));
    }

    #[test]
    fn word_bounds_token() {
        assert_eq!(word_bounds("alpha beta", 2), Some((0, 5)));
        assert_eq!(word_bounds("alpha beta", 6), Some((6, 10)));
        assert_eq!(word_bounds("src/tui/mod.rs", 5), Some((0, 14)));
    }

    #[test]
    fn word_at_screen_cell() {
        let painted = paint(&["hello world"]);
        let sel = painted.word_at("audit", 8, 3).unwrap();
        assert_eq!(painted.extract(&sel).as_deref(), Some("world"));
        assert!(sel.finalized);
    }
}
