//! Right-click popup. Items are verbs; the target is a rail row or sash.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use super::hits::{HitKind, Hits};
use super::theme::{self, Face};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Member(String),
    Space(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Rename,
    Remove,
    Destroy,
}

impl Verb {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rename => "rename",
            Self::Remove => "remove",
            Self::Destroy => "destroy",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Rename => "r",
            Self::Remove => "x",
            Self::Destroy => "d",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub target: Target,
    pub items: Vec<Verb>,
    pub selected: usize,
    pub x: u16,
    pub y: u16,
}

impl Menu {
    pub fn for_member(id: impl Into<String>, x: u16, y: u16) -> Self {
        Self {
            target: Target::Member(id.into()),
            items: vec![Verb::Rename, Verb::Remove, Verb::Destroy],
            selected: 0,
            x,
            y,
        }
    }

    pub fn for_space(name: impl Into<String>, x: u16, y: u16) -> Self {
        Self {
            target: Target::Space(name.into()),
            items: vec![Verb::Rename, Verb::Remove, Verb::Destroy],
            selected: 0,
            x,
            y,
        }
    }

    pub fn title(&self) -> String {
        match &self.target {
            Target::Member(id) => format!(" member {id} "),
            Target::Space(name) => format!(" space {name} "),
        }
    }

    pub fn move_sel(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as i32;
        let next = (self.selected as i32 + delta).rem_euclid(n);
        self.selected = next as usize;
    }

    pub fn current(&self) -> Option<Verb> {
        self.items.get(self.selected).copied()
    }

    pub fn verb_at(&self, i: usize) -> Option<Verb> {
        self.items.get(i).copied()
    }
}

pub fn from_hit(kind: &HitKind, x: u16, y: u16) -> Option<Menu> {
    match kind {
        HitKind::Member(id) | HitKind::Pane(id) => Some(Menu::for_member(id, x, y)),
        HitKind::Workspace(name) | HitKind::Tab(name) => Some(Menu::for_space(name, x, y)),
        _ => None,
    }
}

pub fn rect(menu: &Menu, area: Rect) -> Rect {
    let title = menu.title();
    let inner_w = menu
        .items
        .iter()
        .map(|v| v.label().len() + 4)
        .max()
        .unwrap_or(8)
        .max(title.len().saturating_sub(2));
    let w = (inner_w as u16).saturating_add(2).min(area.width.max(8));
    let h = (menu.items.len() as u16)
        .saturating_add(1)
        .min(area.height.max(2));
    let x = menu.x.min(area.x.saturating_add(area.width.saturating_sub(w)));
    let y = menu.y.min(area.y.saturating_add(area.height.saturating_sub(h)));
    Rect::new(x.max(area.x), y.max(area.y), w, h)
}

pub fn prompt_rect(title: &str, buf: &str, at: Option<(u16, u16)>, area: Rect) -> Rect {
    let inner_w = title
        .len()
        .max(buf.len().saturating_add(1))
        .max(16);
    let w = (inner_w as u16).saturating_add(2).min(area.width.max(8));
    let h = 2u16.min(area.height.max(2));
    let (x, y) = match at {
        Some((px, py)) => (
            px.min(area.x.saturating_add(area.width.saturating_sub(w))),
            py.min(area.y.saturating_add(area.height.saturating_sub(h))),
        ),
        None => (
            area.x.saturating_add(area.width.saturating_sub(w) / 2),
            area.y.saturating_add(area.height.saturating_sub(h) / 2),
        ),
    };
    Rect::new(x.max(area.x), y.max(area.y), w, h)
}

pub fn draw_prompt(
    frame: &mut Frame,
    title: &str,
    buf: &str,
    at: Option<(u16, u16)>,
    hits: &mut Hits,
) {
    let th = theme::t();
    let box_area = prompt_rect(title, buf, at, frame.area());
    frame.render_widget(Clear, box_area);
    frame.render_widget(Block::default().style(th.style(Face::Menu)), box_area);
    let title_row = Rect::new(box_area.x, box_area.y, box_area.width, 1);
    frame.render_widget(
        Paragraph::new(Span::styled(format!(" {title}"), th.style(Face::MenuTitle))),
        title_row,
    );
    if box_area.height >= 2 {
        let input = Rect::new(
            box_area.x,
            box_area.y.saturating_add(1),
            box_area.width,
            1,
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {buf}"), th.style(Face::Menu)),
                Span::styled("_", th.style(Face::MenuActive)),
            ])),
            input,
        );
    }
    hits.push(box_area, HitKind::Prompt);
}

pub fn draw(frame: &mut Frame, menu: &Menu, hits: &mut Hits) {
    let th = theme::t();
    let area = frame.area();
    let box_area = rect(menu, area);
    frame.render_widget(Clear, box_area);
    frame.render_widget(Block::default().style(th.style(Face::Menu)), box_area);
    frame.render_widget(
        Paragraph::new(Span::styled(menu.title(), th.style(Face::MenuTitle))),
        Rect::new(box_area.x, box_area.y, box_area.width, 1),
    );
    let inner = Rect::new(
        box_area.x,
        box_area.y.saturating_add(1),
        box_area.width,
        box_area.height.saturating_sub(1),
    );
    for (i, verb) in menu.items.iter().enumerate() {
        let y = inner.y.saturating_add(i as u16);
        if y >= inner.y.saturating_add(inner.height) {
            break;
        }
        let row = Rect::new(inner.x, y, inner.width, 1);
        let on = i == menu.selected;
        let face = if on { Face::MenuActive } else { Face::Menu };
        let line = Line::from(vec![
            Span::styled(format!(" {} ", verb.hint()), th.style(Face::MenuKey)),
            Span::styled(verb.label(), th.style(face)),
        ]);
        frame.render_widget(Paragraph::new(line).style(th.style(face)), row);
        hits.push(row, HitKind::Menu(i));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_menu_has_the_three_verbs() {
        let m = Menu::for_member("bash", 4, 8);
        assert_eq!(m.items, vec![Verb::Rename, Verb::Remove, Verb::Destroy]);
        assert!(matches!(m.target, Target::Member(id) if id == "bash"));
    }

    #[test]
    fn hit_on_a_space_or_tab_opens_space_menu() {
        let m = from_hit(&HitKind::Workspace("fleet-os".into()), 0, 0).unwrap();
        assert!(matches!(m.target, Target::Space(n) if n == "fleet-os"));
        let m = from_hit(&HitKind::Tab("home".into()), 1, 1).unwrap();
        assert!(matches!(m.target, Target::Space(n) if n == "home"));
        assert!(from_hit(&HitKind::Compose, 0, 0).is_none());
    }

    #[test]
    fn rect_stays_on_screen() {
        let m = Menu::for_member("notes", 100, 40);
        let r = rect(&m, Rect::new(0, 0, 40, 12));
        assert!(r.x + r.width <= 40);
        assert!(r.y + r.height <= 12);
        assert!(r.height >= 4);
    }

    #[test]
    fn prompt_clamps_and_centers() {
        let area = Rect::new(0, 0, 40, 12);
        let at = prompt_rect("rename sash", "default", Some((100, 40)), area);
        assert!(at.x + at.width <= 40);
        assert!(at.y + at.height <= 12);
        assert_eq!(at.height, 2);
        let mid = prompt_rect("rename sash", "default", None, area);
        assert!(mid.x > 0);
        assert!(mid.y > 0);
    }
}
