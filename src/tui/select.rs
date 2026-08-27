//! Drag-select on a pane's grid. Copy-on-select is chrome, not a kernel word.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::daemon::pane::Grid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub col: u16,
    pub row: u16,
}

/// A region inside one pane, in the pane's 0-based cell coordinates.
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

/// Inclusive cell range, trailing pad trimmed, empty if nothing remains.
pub fn extract(grid: &Grid, sel: &Selection) -> Option<String> {
    let (a, b) = sel.ordered();
    let mut out = Vec::new();
    for row in a.row..=b.row {
        let line = grid.lines.get(row as usize).cloned().unwrap_or_default();
        let start = if row == a.row { a.col } else { 0 };
        let end = if row == b.row {
            b.col.saturating_add(1)
        } else {
            line.chars().count() as u16
        };
        out.push(slice_chars(&line, start, end).trim_end().to_string());
    }
    let text = out.join("\n");
    let trimmed = text.trim_end_matches('\n');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn highlight(lines: &[Line<'static>], sel: &Selection, hi: Style) -> Vec<Line<'static>> {
    let (a, b) = sel.ordered();
    let hi = hi.add_modifier(Modifier::REVERSED);
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let row = i as u16;
            if row < a.row || row > b.row {
                return line.clone();
            }
            let start = if row == a.row { a.col } else { 0 };
            let end = if row == b.row {
                b.col.saturating_add(1)
            } else {
                u16::MAX
            };
            highlight_line(line, start, end, hi)
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
            let style = if col >= start && col < end {
                hi
            } else {
                span.style
            };
            push_char(&mut spans, ch, style);
            col = col.saturating_add(1);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(lines: &[&str]) -> Grid {
        Grid {
            cols: 20,
            rows: lines.len() as u16,
            cursor_col: 0,
            cursor_row: 0,
            lines: lines.iter().map(|s| (*s).to_string()).collect(),
            runs: vec![],
            alive: true,
            acp: false,
            mouse: false,
            kitty: 0,
            modify: false,
            alternate: false,
            scroll: 0,
        }
    }

    #[test]
    fn drag_extracts_inclusive_range() {
        let g = grid(&["alpha beta", "gamma"]);
        let mut sel = Selection::begin("1", 0, 0);
        sel.drag(4, 0);
        assert_eq!(extract(&g, &sel).as_deref(), Some("alpha"));
        sel.drag(4, 1);
        assert_eq!(extract(&g, &sel).as_deref(), Some("alpha beta\ngamma"));
    }

    #[test]
    fn click_is_one_cell() {
        let sel = Selection::begin("1", 2, 0);
        assert!(sel.was_just_click());
        let g = grid(&["alpha"]);
        assert_eq!(extract(&g, &sel).as_deref(), Some("p"));
    }

    #[test]
    fn trailing_pad_is_trimmed() {
        let g = grid(&["hi        "]);
        let mut sel = Selection::begin("1", 0, 0);
        sel.drag(9, 0);
        assert_eq!(extract(&g, &sel).as_deref(), Some("hi"));
    }
}
