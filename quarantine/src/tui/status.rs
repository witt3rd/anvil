//! Per-smith-pane status strip. Each session has its own cwd / git /
//! model / context. Widgets are named seats; `clock` still reads the
//! casing `casing.status` mount and only paints on the focused pane.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, Paragraph};
use ratatui::Frame;

use super::theme::{self, Face};
use super::App;
use crate::stats;

#[derive(Debug, Clone, Default)]
pub struct GitSnap {
    pub branch: String,
    pub dirty: bool,
}

pub type GitCache = HashMap<PathBuf, (Instant, GitSnap)>;

pub fn refresh_git(cwd: &Path, cache: &mut GitCache) -> GitSnap {
    if let Some((at, snap)) = cache.get(cwd) {
        if at.elapsed() < Duration::from_secs(2) {
            return snap.clone();
        }
    }
    let snap = probe_git(cwd);
    cache.insert(cwd.to_path_buf(), (Instant::now(), snap.clone()));
    snap
}

pub fn cwd_for(app: &App, session: &str) -> PathBuf {
    app.frame
        .as_ref()
        .and_then(|r| r.session(session).ok())
        .and_then(|s| s.meta.cwd)
        .map(PathBuf::from)
        .unwrap_or_else(|| app.cwd.clone())
}

fn probe_git(cwd: &Path) -> GitSnap {
    let out = Command::new("git")
        .args(["-C", &cwd.display().to_string(), "status", "-sb"])
        .output();
    let Ok(out) = out else {
        return GitSnap::default();
    };
    if !out.status.success() {
        return GitSnap::default();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next().unwrap_or("");
    let branch = first
        .trim()
        .trim_start_matches("## ")
        .split("...")
        .next()
        .unwrap_or("")
        .to_string();
    let dirty = text.lines().nth(1).is_some();
    GitSnap { branch, dirty }
}

pub fn compact_cwd(cwd: &Path) -> String {
    let raw = cwd.display().to_string();
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = raw.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    raw
}

pub fn context_fill(app: &App, session: &str) -> Option<(u32, u32, f64)> {
    let fold = if session == app.session_id() && !app.log_events.is_empty() {
        stats::fold(&app.log_events, "")
    } else {
        let root = app.frame.as_ref()?;
        let events = root.load_events(session).ok()?;
        stats::fold(&events, "")
    };
    let used = fold.context.projected.or(fold.context.pressure)?;
    let win = fold.context.window.or(app.context_window)?;
    if win == 0 {
        return None;
    }
    Some((used, win, f64::from(used) / f64::from(win)))
}

fn ctx_face(frac: f64) -> Face {
    if frac >= 0.9 {
        Face::PlotGaugeHot
    } else if frac >= 0.75 {
        Face::PlotGaugeWarn
    } else {
        Face::PlotGauge
    }
}

pub fn draw(frame: &mut Frame, area: Rect, app: &App, session: &str) {
    let th = theme::t();
    let sep = Span::styled(" │ ", th.style(Face::StatusSep));
    let mut spans = vec![Span::styled(" ", th.style(Face::StatusBar))];
    let mut first = true;
    for name in &app.status_widgets {
        let Some((face, text)) = widget(app, name, session) else {
            continue;
        };
        if !first {
            spans.push(sep.clone());
        }
        first = false;
        spans.push(Span::styled(text, th.style(face)));
    }
    let trail = area.width.saturating_sub(
        spans
            .iter()
            .map(|s| s.content.chars().count() as u16)
            .sum::<u16>(),
    );
    if trail > 0 {
        spans.push(Span::styled(
            " ".repeat(trail as usize),
            th.style(Face::StatusBar),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(th.style(Face::StatusBar)),
        area,
    );
}

pub fn widget(app: &App, name: &str, session: &str) -> Option<(Face, String)> {
    let meta = app.frame.as_ref().and_then(|r| r.session(session).ok());
    match name {
        "spin" => {
            let live = session == app.session_id();
            let spin = if live && app.busy {
                ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"][(app.tick as usize) % 8]
            } else {
                "·"
            };
            let text = if live {
                format!("{spin} {}", app.status)
            } else {
                spin.to_string()
            };
            Some((Face::StatusInk, text))
        }
        "focus" => {
            let focus = if session == app.session_id() {
                match (app.focus, app.focused_is_pty(), app.focused_is_edit()) {
                    (super::rail::Focus::Rail, _, _) => "rail",
                    (super::rail::Focus::Compose, true, _) => "pty",
                    (super::rail::Focus::Compose, _, true) => "edit",
                    (super::rail::Focus::Compose, false, false) => "ask",
                }
            } else {
                "idle"
            };
            Some((Face::StatusInk, focus.into()))
        }
        "account" => {
            let name = meta
                .as_ref()
                .and_then(|s| s.meta.provider.clone())
                .unwrap_or_else(|| app.provider_name.clone());
            Some((Face::StatusInk, name))
        }
        "cwd" => Some((Face::StatusInk, compact_cwd(&cwd_for(app, session)))),
        "git" => {
            let cwd = cwd_for(app, session);
            let git = app.git_cache.get(&cwd).map(|(_, s)| s.clone())?;
            if git.branch.is_empty() {
                return None;
            }
            let face = if git.dirty {
                Face::PlotGaugeWarn
            } else {
                Face::StatusGit
            };
            Some((face, git.branch))
        }
        "model" => {
            let name = meta
                .as_ref()
                .and_then(|s| s.meta.model.clone())
                .unwrap_or_else(|| app.model.clone());
            Some((Face::StatusInk, name))
        }
        "context" => {
            let (used, win, frac) = context_fill(app, session)?;
            Some((
                ctx_face(frac),
                format!(
                    "{}% {}/{}",
                    (frac * 100.0).round() as u16,
                    stats::fmt_tokens(used),
                    stats::fmt_tokens(win)
                ),
            ))
        }
        "clock" => {
            if session != app.session_id() {
                return None;
            }
            let clock = app.slot_status.as_deref().filter(|s| !s.is_empty())?;
            Some((Face::StatusInk, clock.to_string()))
        }
        _ => None,
    }
}

#[allow(dead_code)]
pub fn draw_progress(frame: &mut Frame, area: Rect, app: &App) {
    let th = theme::t();
    let label = app
        .activity
        .as_ref()
        .map(|a| a.chip())
        .unwrap_or_else(|| format!("⋮ {}", app.status));
    let frac = context_fill(app, &app.session_id())
        .map(|(_, _, f)| f.clamp(0.02, 1.0))
        .unwrap_or_else(|| {
            let t = app
                .activity
                .as_ref()
                .map(|a| a.elapsed().as_secs_f64())
                .unwrap_or(0.0);
            (t / 20.0).clamp(0.08, 0.92)
        });
    let face = ctx_face(frac);
    frame.render_widget(
        Gauge::default()
            .gauge_style(th.style(face))
            .ratio(frac)
            .label(label),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn compact_cwd_uses_tilde() {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/home/dt".into()));
        let p = home.join("src/witt3rd/anvil");
        let s = compact_cwd(&p);
        assert!(s.starts_with("~/"), "{s}");
        assert!(s.contains("anvil"), "{s}");
    }

    #[test]
    fn ctx_face_ramps() {
        assert_eq!(ctx_face(0.2), Face::PlotGauge);
        assert_eq!(ctx_face(0.8), Face::PlotGaugeWarn);
        assert_eq!(ctx_face(0.95), Face::PlotGaugeHot);
    }
}
