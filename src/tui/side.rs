//! The sidebar's two lists: windows above, agent processes below.

use std::path::Path;

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use crate::daemon::acp::WindowState;
use crate::daemon::session::{SessionView, WindowView};

/// Default open width: half of the old 42-cell roster.
pub const DEFAULT_COLS: u16 = 21;
pub const MIN_COLS: u16 = 12;
pub const MAX_COLS: u16 = 60;
const FILE: &str = "sidebar.json";

/// What the client remembers about the sidebar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default = "default_open")]
    pub open: bool,
    #[serde(default = "default_cols")]
    pub cols: u16,
    /// Fraction of the sidebar given to the window list (the rest is agents).
    #[serde(default = "default_split")]
    pub split: f32,
}

fn default_open() -> bool {
    true
}
fn default_cols() -> u16 {
    DEFAULT_COLS
}
fn default_split() -> f32 {
    0.5
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            open: true,
            cols: DEFAULT_COLS,
            split: 0.5,
        }
    }
}

impl Prefs {
    pub fn load(root: &Path) -> Prefs {
        let path = root.join(FILE);
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(mut prefs) = serde_json::from_str::<Prefs>(&text) {
                prefs.cols = prefs.cols.clamp(MIN_COLS, MAX_COLS);
                prefs.split = prefs.split.clamp(0.2, 0.8);
                return prefs;
            }
        }
        Prefs::default()
    }

    pub fn save(&self, root: &Path) {
        let _ = std::fs::create_dir_all(root);
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(root.join(FILE), text);
        }
    }

    pub fn clamp_cols(&self, term_w: u16) -> u16 {
        let max = term_w.saturating_sub(20).max(MIN_COLS).min(MAX_COLS);
        self.cols.clamp(MIN_COLS, max)
    }
}

/// A row the sidebar can focus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideItem {
    Window(String),
    Agent {
        pane: String,
        window: String,
        name: String,
    },
}

impl SideItem {
    pub fn same(&self, other: &SideItem) -> bool {
        match (self, other) {
            (SideItem::Window(a), SideItem::Window(b)) => a == b,
            (SideItem::Agent { pane: a, .. }, SideItem::Agent { pane: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// Hit target in the sidebar column (not the tiles).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideHit {
    Window(String),
    Pane(String),
}

pub struct SideLayout {
    pub hits: Vec<(u16, u16, SideItem)>,
    pub divider_y: Option<u16>,
    pub windows_header: Option<u16>,
    pub agents_header: Option<u16>,
    pub agent_area: Rect,
}

impl SideLayout {
    pub fn at(&self, row: u16) -> Option<&SideItem> {
        self.hits
            .iter()
            .find(|(y, h, _)| row >= *y && row < *y + *h)
            .map(|(_, _, item)| item)
    }

    pub fn hit(&self, row: u16) -> Option<SideHit> {
        match self.at(row)? {
            SideItem::Window(w) => Some(SideHit::Window(w.clone())),
            SideItem::Agent { pane, .. } => Some(SideHit::Pane(pane.clone())),
        }
    }
}

pub fn agents(view: &SessionView) -> Vec<SideItem> {
    view.windows
        .iter()
        .flat_map(|w| {
            w.panes.iter().filter_map(|p| {
                p.name.as_ref().map(|name| SideItem::Agent {
                    pane: p.pane.clone(),
                    window: w.window.clone(),
                    name: name.clone(),
                })
            })
        })
        .collect()
}

pub fn items(view: &SessionView) -> Vec<SideItem> {
    let mut out: Vec<SideItem> = view
        .windows
        .iter()
        .map(|w| SideItem::Window(w.window.clone()))
        .collect();
    out.extend(agents(view));
    out
}

/// Rows reserved at the top of the window list so the rail marks
/// sit on the same rows as the open names. Matches: blank,
/// "windows", blank.
const WIN_HEAD_ROWS: u16 = 3;
/// Rows reserved at the top of the agent list: "agents", blank.
const AGENT_HEAD_ROWS: u16 = 2;
/// Blank row above the footer, in the sidebar only — panes keep
/// their height.
const FOOT_PAD: u16 = 1;

/// The divider follows `split` on the rail and the open sidebar.
pub fn layout(area: Rect, open: bool, split: f32, view: &SessionView) -> SideLayout {
    let area = Rect {
        height: area.height.saturating_sub(FOOT_PAD),
        ..area
    };
    let item_h: u16 = if open { 2 } else { 1 };
    let windows: Vec<SideItem> = view
        .windows
        .iter()
        .map(|w| SideItem::Window(w.window.clone()))
        .collect();
    let agent_items = agents(view);
    let (win_area, divider_y, agent_area) = sections(area, open, split);

    let mut hits = Vec::new();
    let (mut y, windows_header) = list_start(win_area.y, win_area.bottom(), open, true);
    for item in windows {
        if y + item_h > win_area.bottom() {
            break;
        }
        hits.push((y, item_h, item));
        y += item_h;
    }

    // The divider is already air above "agents" — no extra blank row.
    let (mut y, agents_header) = list_start(agent_area.y, agent_area.bottom(), open, false);
    for item in agent_items {
        if y + item_h > agent_area.bottom() {
            break;
        }
        hits.push((y, item_h, item));
        y += item_h;
    }

    SideLayout {
        hits,
        divider_y,
        windows_header,
        agents_header,
        agent_area,
    }
}

/// Open: blank / label / blank (windows) or label / blank (agents).
/// Rail: skip the same rows so marks do not jump when the list closes.
fn list_start(top: u16, bottom: u16, open: bool, windows: bool) -> (u16, Option<u16>) {
    let head = if windows { WIN_HEAD_ROWS } else { AGENT_HEAD_ROWS };
    if !open {
        let y = (top + head).min(bottom.saturating_sub(1).max(top));
        return (y, None);
    }
    let mut y = top;
    if windows && y < bottom {
        y += 1;
    }
    if y >= bottom {
        return (y, None);
    }
    let header = y;
    y += 1;
    if y < bottom {
        y += 1;
    }
    let _ = head;
    (y, Some(header))
}

fn sections(area: Rect, open: bool, split: f32) -> (Rect, Option<u16>, Rect) {
    if area.height < 3 {
        return (area, None, Rect::default());
    }
    let item_h: u16 = if open { 2 } else { 1 };
    let min_win = WIN_HEAD_ROWS + item_h;
    let min_agent = AGENT_HEAD_ROWS.max(1);
    let usable = area.height.saturating_sub(1);
    let want = ((usable as f32) * split.clamp(0.2, 0.8)).round() as u16;
    let win_h = want.clamp(min_win, usable.saturating_sub(min_agent)).max(1);
    let divider_y = area.y + win_h;
    let agent_y = divider_y + 1;
    let agent_h = area.bottom().saturating_sub(agent_y);
    (
        Rect::new(area.x, area.y, area.width, win_h),
        Some(divider_y),
        Rect::new(area.x, agent_y, area.width, agent_h),
    )
}

pub fn window_clause(window: &WindowView) -> String {
    let named: Vec<&str> = window
        .panes
        .iter()
        .filter_map(|p| p.name.as_deref())
        .collect();
    let shells = window.panes.len().saturating_sub(named.len());
    match (named.as_slice(), shells) {
        ([], 1) => "shell".into(),
        ([], n) => format!("{n} shells"),
        ([a], 0) => (*a).to_string(),
        ([a], _) => format!("{a} · shell"),
        (many, 0) => many.join(" · "),
        (many, _) => format!("{} · shell", many.join(" · ")),
    }
}

pub fn agent_clause(window: &str, state: WindowState) -> String {
    match state {
        WindowState::Turning => format!("{window} · turning"),
        WindowState::NeedsYou => format!("{window} · needs you"),
        WindowState::Dead => format!("{window} · dead"),
        WindowState::Idle => window.to_string(),
    }
}

pub fn window_has_agent(window: &WindowView) -> bool {
    window.panes.iter().any(|p| p.name.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::session::PaneView;

    fn view() -> SessionView {
        SessionView {
            focused: "1".into(),
            windows: vec![
                WindowView {
                    window: "ansible".into(),
                    state: WindowState::Idle,
                    panes: vec![
                        PaneView {
                            pane: "1".into(),
                            x: 0,
                            y: 0,
                            cols: 20,
                            rows: 10,
                            name: Some("oc".into()),
                            state: WindowState::Idle,
                        },
                        PaneView {
                            pane: "2".into(),
                            x: 21,
                            y: 0,
                            cols: 20,
                            rows: 10,
                            name: None,
                            state: WindowState::Idle,
                        },
                    ],
                },
                WindowView {
                    window: "sh".into(),
                    state: WindowState::Idle,
                    panes: vec![PaneView {
                        pane: "3".into(),
                        x: 0,
                        y: 0,
                        cols: 40,
                        rows: 10,
                        name: None,
                        state: WindowState::Idle,
                    }],
                },
            ],
        }
    }

    #[test]
    fn shell_windows_are_not_agents() {
        let v = view();
        let agent_list = agents(&v);
        assert_eq!(agent_list.len(), 1);
        assert!(matches!(
            &agent_list[0],
            SideItem::Agent { name, window, .. } if name == "oc" && window == "ansible"
        ));
    }

    #[test]
    fn window_clause_names_what_is_on_it() {
        let v = view();
        assert_eq!(window_clause(&v.windows[0]), "oc · shell");
        assert_eq!(window_clause(&v.windows[1]), "shell");
    }

    #[test]
    fn open_sidebar_splits_on_the_ratio() {
        let v = view();
        let lay = layout(Rect::new(0, 0, 21, 24), true, 0.5, &v);
        assert_eq!(lay.hits[0].1, 2);
        assert_eq!(lay.windows_header, Some(1));
        assert_eq!(lay.at(3), Some(&SideItem::Window("ansible".into())));
        assert_eq!(lay.divider_y, Some(11));
        assert_eq!(lay.agents_header, Some(12));
        assert!(matches!(lay.at(14), Some(SideItem::Agent { name, .. }) if name == "oc"));
    }

    #[test]
    fn empty_agents_section_still_shows() {
        let v = SessionView {
            focused: "1".into(),
            windows: vec![WindowView {
                window: "sh".into(),
                state: WindowState::Idle,
                panes: vec![PaneView {
                    pane: "1".into(),
                    x: 0,
                    y: 0,
                    cols: 40,
                    rows: 10,
                    name: None,
                    state: WindowState::Idle,
                }],
            }],
        };
        let lay = layout(Rect::new(0, 0, 21, 24), true, 0.5, &v);
        assert_eq!(lay.divider_y, Some(11));
        assert_eq!(lay.agents_header, Some(12));
        assert!(agents(&v).is_empty());
        assert!(lay.at(14).is_none());
    }

    #[test]
    fn rail_keeps_the_same_split() {
        let v = view();
        let open = layout(Rect::new(0, 0, 21, 20), true, 0.5, &v);
        let rail = layout(Rect::new(0, 0, 3, 20), false, 0.5, &v);
        assert_eq!(open.divider_y, rail.divider_y);
        assert_eq!(rail.divider_y, Some(9));
        assert_eq!(rail.at(3), Some(&SideItem::Window("ansible".into())));
        assert_eq!(open.at(3), Some(&SideItem::Window("ansible".into())));
        assert!(matches!(rail.at(12), Some(SideItem::Agent { name, .. }) if name == "oc"));
        assert!(rail.at(0).is_none());
        assert!(rail.at(9).is_none());
    }
}
