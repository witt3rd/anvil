//! Session list rows for the prefix-s popup.

use crate::daemon::acp::WindowState;
use crate::daemon::session::SessionView;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub title: String,
    pub clause: String,
}

pub fn row(index: usize, name: &str, view: &SessionView) -> Row {
    Row {
        name: name.to_string(),
        title: crate::tui::display_session(name, index),
        clause: clause(view),
    }
}

pub fn clause(view: &SessionView) -> String {
    if view.windows.is_empty() {
        return "empty".into();
    }
    let mut bits = Vec::new();
    for w in &view.windows {
        let dead = w.state == WindowState::Dead
            || (!w.panes.is_empty() && w.panes.iter().all(|p| p.state == WindowState::Dead));
        let activity = w.panes.iter().find_map(|p| p.activity.as_deref());
        let agents: Vec<&str> = w.panes.iter().filter_map(|p| p.name.as_deref()).collect();
        let label = if let Some(act) = activity {
            act.to_string()
        } else if agents.is_empty() {
            w.window.clone()
        } else {
            agents.join(" · ")
        };
        bits.push(if dead {
            format!("{label} (dead)")
        } else if w.state == WindowState::NeedsYou
            || w.panes.iter().any(|p| p.state == WindowState::NeedsYou)
        {
            format!("{label} (needs you)")
        } else if w.state == WindowState::Turning
            || w.panes.iter().any(|p| p.state == WindowState::Turning)
        {
            format!("{label} (turning)")
        } else {
            label
        });
    }
    bits.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::session::{PaneView, WindowView};

    #[test]
    fn clause_names_agents_and_dead_shells() {
        let view = SessionView {
            focused: "1".into(),
            windows: vec![
                WindowView {
                    window: "oc".into(),
                    state: WindowState::Idle,
                    note: String::new(),
                    panes: vec![PaneView {
                        pane: "1".into(),
                        x: 0,
                        y: 0,
                        cols: 10,
                        rows: 10,
                        name: Some("oc".into()),
                        activity: None,
                        session: None,
                        cwd: None,
                        state: WindowState::Idle,
                    }],
                },
                WindowView {
                    window: "sh".into(),
                    state: WindowState::Dead,
                    note: String::new(),
                    panes: vec![PaneView {
                        pane: "2".into(),
                        x: 0,
                        y: 0,
                        cols: 10,
                        rows: 10,
                        name: None,
                        activity: None,
                        session: None,
                        cwd: None,
                        state: WindowState::Dead,
                    }],
                },
            ],
        };
        assert_eq!(clause(&view), "oc · sh (dead)");
    }
}
