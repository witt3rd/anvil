//! Last-paint hit map. Draw records every clickable surface; mouse
//! looks up (col, row) instead of re-deriving layout.

use ratatui::layout::Rect;

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
    Compose,
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
                    HitKind::Compose => None,
                    other => Some(other),
                }
            })
            .or_else(|| self.at(col, row))
    }
}

pub fn inside(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && y >= r.y && x < r.x.saturating_add(r.width) && y < r.y.saturating_add(r.height)
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
}
