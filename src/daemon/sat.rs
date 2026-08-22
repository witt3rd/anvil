//! Saturation: how much of the named-agent fleet is turning.
//! Chrome, sampled here so detach does not reset the clock.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::acp::WindowState;
use super::session::{SessionView, Sessions};

const FILE: &str = "saturation.json";
pub const SAMPLE_MS: u64 = 5_000;
const BUCKET_MS: u64 = 5 * 60 * 1_000;
const RING: usize = 288;

/// One scope: the box, or one session.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Counters {
    pub busy: u32,
    pub agents: u32,
    #[serde(default)]
    pub work_ms_24h: u64,
    #[serde(default)]
    pub span_ms_24h: u64,
    #[serde(default)]
    pub work_ms_all: u64,
    #[serde(default)]
    pub span_ms_all: u64,
    #[serde(default)]
    bucket_ms: u64,
    #[serde(default)]
    bucket_work: u64,
    #[serde(default)]
    bucket_span: u64,
    #[serde(default)]
    buckets: Vec<(u64, u64)>,
}

impl Counters {
    pub fn snapshot(busy: u32, agents: u32) -> Counters {
        Counters {
            busy,
            agents,
            ..Counters::default()
        }
    }

    pub fn instant(&self) -> Option<f32> {
        proportion(self.busy, self.agents)
    }

    pub fn mean_24h(&self) -> Option<f32> {
        if self.span_ms_24h == 0 {
            None
        } else {
            Some(self.work_ms_24h as f32 / self.span_ms_24h as f32)
        }
    }
}

/// Named agent panes. Busy is `turning` only.
pub fn count_view(view: &SessionView) -> (u32, u32) {
    let mut busy = 0;
    let mut agents = 0;
    for window in &view.windows {
        for pane in &window.panes {
            if pane.name.is_none() {
                continue;
            }
            agents += 1;
            if pane.state == WindowState::Turning {
                busy += 1;
            }
        }
    }
    (busy, agents)
}

pub fn proportion(busy: u32, agents: u32) -> Option<f32> {
    if agents == 0 {
        None
    } else {
        Some(busy as f32 / agents as f32)
    }
}

/// Energy band from fleet size. 1 is the nucleus; each doubling
/// of agents is another shell. 0 means no saturation to draw.
pub fn band(agents: u32) -> u8 {
    if agents == 0 {
        0
    } else {
        (u32::BITS - agents.leading_zeros()) as u8
    }
}

/// What the client paints. Daemon writes, client reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snap {
    pub all: Counters,
    #[serde(default)]
    pub sessions: BTreeMap<String, Counters>,
    #[serde(default)]
    pub ring: Vec<f32>,
    #[serde(default)]
    sampled_at: u64,
}

impl Snap {
    pub fn load(root: &Path) -> Snap {
        let path = root.join(FILE);
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(snap) = serde_json::from_str(&text) {
                return snap;
            }
        }
        Snap::default()
    }

    pub fn save(&self, root: &Path) {
        let _ = std::fs::create_dir_all(root);
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(root.join(FILE), text);
        }
    }

    pub fn session(&self, name: &str) -> Option<&Counters> {
        self.sessions.get(name)
    }

    /// Credit `dt_ms` of the current snapshot. The clock runs only
    /// while a scope has at least one agent. `keep` is the live
    /// session list — names not on it drop out.
    pub fn apply(&mut self, shots: &[(String, u32, u32)], dt_ms: u64, keep: &[String]) {
        let dt = dt_ms.min(SAMPLE_MS * 2);
        let mut all_busy = 0;
        let mut all_agents = 0;
        for (name, busy, agents) in shots {
            all_busy += *busy;
            all_agents += *agents;
            credit(self.sessions.entry(name.clone()).or_default(), *busy, *agents, dt);
        }
        let pushed = credit(&mut self.all, all_busy, all_agents, dt);
        if let Some(frac) = pushed {
            self.ring.push(frac);
            if self.ring.len() > RING {
                self.ring.remove(0);
            }
        }
        self.sessions.retain(|name, _| keep.iter().any(|k| k == name));
        self.sampled_at = now_ms();
    }
}

fn credit(c: &mut Counters, busy: u32, agents: u32, dt: u64) -> Option<f32> {
    c.busy = busy;
    c.agents = agents;
    if agents == 0 || dt == 0 {
        return None;
    }
    let work = dt * busy as u64;
    let span = dt * agents as u64;
    c.work_ms_all += work;
    c.span_ms_all += span;
    c.bucket_work += work;
    c.bucket_span += span;
    c.bucket_ms += dt;
    let mut closed = None;
    if c.bucket_ms >= BUCKET_MS {
        let frac = if c.bucket_span == 0 {
            0.0
        } else {
            c.bucket_work as f32 / c.bucket_span as f32
        };
        c.buckets.push((c.bucket_work, c.bucket_span));
        if c.buckets.len() > RING {
            c.buckets.remove(0);
        }
        c.bucket_ms = 0;
        c.bucket_work = 0;
        c.bucket_span = 0;
        closed = Some(frac);
    }
    let (w, s) = c.buckets.iter().fold(
        (c.bucket_work, c.bucket_span),
        |acc, b| (acc.0 + b.0, acc.1 + b.1),
    );
    c.work_ms_24h = w;
    c.span_ms_24h = s;
    closed
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Sample live sessions until the daemon process ends.
pub fn run(root: PathBuf, sessions: Arc<Sessions>) {
    tick(&root, &sessions);
    loop {
        thread::sleep(Duration::from_millis(SAMPLE_MS));
        tick(&root, &sessions);
    }
}

fn tick(root: &Path, sessions: &Sessions) {
    let mut shots = Vec::new();
    sessions.each_live_view(|name, view| {
        let (busy, agents) = count_view(&view);
        shots.push((name.to_string(), busy, agents));
    });
    let keep = sessions.list();
    let mut snap = Snap::load(root);
    let dt = if snap.sampled_at == 0 {
        SAMPLE_MS
    } else {
        now_ms().saturating_sub(snap.sampled_at)
    };
    snap.apply(&shots, dt, &keep);
    snap.save(root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::session::{PaneView, WindowView};

    fn pane(name: Option<&str>, state: WindowState) -> PaneView {
        PaneView {
            pane: "1".into(),
            x: 0,
            y: 0,
            cols: 10,
            rows: 10,
            name: name.map(str::to_string),
            activity: None,
            state,
        }
    }

    fn view(panes: Vec<PaneView>) -> SessionView {
        SessionView {
            focused: "1".into(),
            windows: vec![WindowView {
                window: "w".into(),
                state: WindowState::Idle,
                panes,
            }],
        }
    }

    #[test]
    fn shells_are_not_agents() {
        assert_eq!(
            count_view(&view(vec![pane(None, WindowState::Turning)])),
            (0, 0)
        );
    }

    #[test]
    fn needs_you_and_dead_are_holes() {
        let v = view(vec![
            pane(Some("oc"), WindowState::Turning),
            pane(Some("grok"), WindowState::NeedsYou),
            pane(Some("oc-work"), WindowState::Dead),
            pane(Some("idle"), WindowState::Idle),
        ]);
        assert_eq!(count_view(&v), (1, 4));
    }

    #[test]
    fn zero_agents_has_no_proportion() {
        assert_eq!(proportion(0, 0), None);
        assert_eq!(proportion(0, 3), Some(0.0));
        assert_eq!(proportion(3, 3), Some(1.0));
    }

    #[test]
    fn bands_are_shells() {
        assert_eq!(band(0), 0);
        assert_eq!(band(1), 1);
        assert_eq!(band(2), 2);
        assert_eq!(band(3), 2);
        assert_eq!(band(4), 3);
        assert_eq!(band(7), 3);
        assert_eq!(band(8), 4);
        assert_eq!(band(100), 7);
    }

    #[test]
    fn clock_pauses_when_no_agents() {
        let mut snap = Snap::default();
        snap.apply(&[("spire".into(), 0, 0)], SAMPLE_MS, &["spire".into()]);
        assert_eq!(snap.all.span_ms_all, 0);
        assert_eq!(snap.all.agents, 0);
        snap.apply(&[("spire".into(), 1, 1)], SAMPLE_MS, &["spire".into()]);
        assert_eq!(snap.all.span_ms_all, SAMPLE_MS);
        assert_eq!(snap.all.work_ms_all, SAMPLE_MS);
        snap.apply(&[("spire".into(), 0, 0)], SAMPLE_MS, &["spire".into()]);
        assert_eq!(snap.all.span_ms_all, SAMPLE_MS);
        assert_eq!(snap.all.agents, 0);
    }

    #[test]
    fn half_busy_credits_half_work() {
        let mut snap = Snap::default();
        snap.apply(&[("a".into(), 1, 2)], SAMPLE_MS, &["a".into()]);
        assert_eq!(snap.all.work_ms_all, SAMPLE_MS);
        assert_eq!(snap.all.span_ms_all, SAMPLE_MS * 2);
        assert!((snap.all.instant().unwrap() - 0.5).abs() < f32::EPSILON);
        assert!((snap.all.mean_24h().unwrap() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn gone_sessions_drop_out() {
        let mut snap = Snap::default();
        snap.apply(&[("old".into(), 1, 1)], SAMPLE_MS, &["old".into()]);
        snap.apply(&[("new".into(), 1, 1)], SAMPLE_MS, &["new".into()]);
        assert!(snap.sessions.contains_key("new"));
        assert!(!snap.sessions.contains_key("old"));
    }
}
