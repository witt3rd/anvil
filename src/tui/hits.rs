//! Last-paint hit map. Draw records every clickable surface; mouse
//! looks up (col, row) instead of re-deriving layout.

use ratatui::layout::Rect;

use crate::frame::SplitDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitKind {
    Rail,
    Catalog,
    Workspace(String),
    Member(String),
    Tab(String),
    TabAdd,
    SashPrev,
    SashNext,
    Pane(String),
    /// Shared border between two siblings. `path` is from the workspace
    /// tile root (`None` = linear stage stack). `sizes` are the painted
    /// along-axis cell counts of every sibling in that split.
    SplitEdge {
        path: Option<Vec<usize>>,
        gap: usize,
        dir: SplitDir,
        sizes: Vec<u16>,
    },
    Compose,
    PasteChip(usize),
    Picker(usize),
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub area: Rect,
    pub kind: HitKind,
}

#[derive(Debug, Clone, Default)]
pub struct Hits {
    targets: Vec<Hit>,
}

impl Hits {
    pub fn push(&mut self, area: Rect, kind: HitKind) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.targets.push(Hit { area, kind });
    }

    /// Last recorded match wins so nested surfaces (a tab on the sash,
    /// compose inside a pane) beat the region they sit on.
    pub fn at(&self, col: u16, row: u16) -> Option<&HitKind> {
        self.targets
            .iter()
            .rev()
            .find(|h| inside(h.area, col, row))
            .map(|h| &h.kind)
    }

    pub fn pane_area(&self, id: &str) -> Option<Rect> {
        self.targets.iter().rev().find_map(|h| match &h.kind {
            HitKind::Pane(p) if p == id => Some(h.area),
            _ => None,
        })
    }

    pub fn nearest_pane(&self, from: Rect, dir: NavDir) -> Option<String> {
        let (fx, fy) = center(from);
        let mut best: Option<(u32, String)> = None;
        let mut seen = std::collections::HashSet::new();
        for h in &self.targets {
            let HitKind::Pane(id) = &h.kind else {
                continue;
            };
            if h.area == from || !seen.insert(id.clone()) {
                continue;
            }
            let (cx, cy) = center(h.area);
            let dx = cx as i32 - fx as i32;
            let dy = cy as i32 - fy as i32;
            let along = match dir {
                NavDir::Left => -dx,
                NavDir::Right => dx,
                NavDir::Up => -dy,
                NavDir::Down => dy,
            };
            if along <= 0 {
                continue;
            }
            let across = match dir {
                NavDir::Left | NavDir::Right => dy.unsigned_abs(),
                NavDir::Up | NavDir::Down => dx.unsigned_abs(),
            };
            let score = along as u32 * 2 + across;
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((score, id.clone()));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Wheel target: the pane under the pointer, even if compose/chip
    /// sits on top of it. Falls back to the finest hit.
    pub fn at_scroll(&self, col: u16, row: u16) -> Option<&HitKind> {
        self.targets
            .iter()
            .rev()
            .find_map(|h| {
                if !inside(h.area, col, row) {
                    return None;
                }
                match &h.kind {
                    HitKind::Compose | HitKind::PasteChip(_) | HitKind::SplitEdge { .. } => None,
                    other => Some(other),
                }
            })
            .or_else(|| self.at(col, row))
    }
}

/// One- or two-cell sash between adjacent tiled panes.
pub fn split_edge_rect(dir: SplitDir, a: Rect, b: Rect) -> Rect {
    match dir {
        SplitDir::Col => {
            let y = a.y.saturating_add(a.height.saturating_sub(1));
            let bottom = b.y.saturating_add(1);
            let h = bottom.saturating_sub(y).clamp(1, 2);
            let x = a.x.max(b.x);
            let right = a
                .x
                .saturating_add(a.width)
                .min(b.x.saturating_add(b.width));
            Rect::new(x, y, right.saturating_sub(x), h)
        }
        SplitDir::Row => {
            let x = a.x.saturating_add(a.width.saturating_sub(1));
            let right = b.x.saturating_add(1);
            let w = right.saturating_sub(x).clamp(1, 2);
            let y = a.y.max(b.y);
            let bottom = a
                .y
                .saturating_add(a.height)
                .min(b.y.saturating_add(b.height));
            Rect::new(x, y, w, bottom.saturating_sub(y))
        }
    }
}

pub fn inside(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && y >= r.y && x < r.x.saturating_add(r.width) && y < r.y.saturating_add(r.height)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDir {
    Left,
    Right,
    Up,
    Down,
}

fn center(r: Rect) -> (u16, u16) {
    (
        r.x.saturating_add(r.width / 2),
        r.y.saturating_add(r.height / 2),
    )
}

pub fn row_rect(area: Rect, line: usize) -> Rect {
    let y = area.y.saturating_add(line as u16);
    if y >= area.y.saturating_add(area.height) {
        return Rect::new(0, 0, 0, 0);
    }
    Rect::new(area.x, y, area.width, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::SplitDir;

    #[test]
    fn later_hit_wins_inside_overlap() {
        let mut hits = Hits::default();
        hits.push(Rect::new(0, 0, 20, 10), HitKind::Rail);
        hits.push(Rect::new(0, 3, 20, 1), HitKind::Workspace("fleet".into()));
        hits.push(Rect::new(22, 1, 40, 8), HitKind::Pane("audit".into()));
        hits.push(Rect::new(23, 7, 38, 1), HitKind::Compose);
        assert_eq!(hits.at(2, 1), Some(&HitKind::Rail));
        assert_eq!(hits.at(2, 3), Some(&HitKind::Workspace("fleet".into())));
        assert_eq!(hits.at(30, 4), Some(&HitKind::Pane("audit".into())));
        assert_eq!(hits.at(30, 7), Some(&HitKind::Compose));
        assert_eq!(hits.at(80, 0), None);
        assert_eq!(
            hits.at_scroll(30, 7),
            Some(&HitKind::Pane("audit".into())),
            "wheel on compose still scrolls the pane"
        );
        assert_eq!(hits.at_scroll(30, 4), Some(&HitKind::Pane("audit".into())));
    }

    #[test]
    fn row_rect_clips_past_the_area() {
        let area = Rect::new(0, 5, 10, 2);
        assert_eq!(row_rect(area, 0), Rect::new(0, 5, 10, 1));
        assert_eq!(row_rect(area, 1), Rect::new(0, 6, 10, 1));
        assert_eq!(row_rect(area, 2).width, 0);
    }

    #[test]
    fn split_edge_covers_the_shared_border() {
        let top = Rect::new(10, 2, 20, 6);
        let bot = Rect::new(10, 8, 20, 5);
        let col_edge = split_edge_rect(SplitDir::Col, top, bot);
        assert_eq!(col_edge, Rect::new(10, 7, 20, 2));
        let left = Rect::new(0, 1, 12, 8);
        let right = Rect::new(12, 1, 10, 8);
        let row_edge = split_edge_rect(SplitDir::Row, left, right);
        assert_eq!(row_edge, Rect::new(11, 1, 2, 8));
        let mut hits = Hits::default();
        hits.push(top, HitKind::Pane("a".into()));
        hits.push(bot, HitKind::Pane("b".into()));
        hits.push(
            col_edge,
            HitKind::SplitEdge {
                path: Some(vec![]),
                gap: 0,
                dir: SplitDir::Col,
                sizes: vec![6, 5],
            },
        );
        assert!(matches!(
            hits.at(15, 7),
            Some(HitKind::SplitEdge { gap: 0, .. })
        ));
        assert_eq!(hits.at_scroll(15, 7), Some(&HitKind::Pane("a".into())));
    }
}
