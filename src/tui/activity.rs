//! Live ask activity: a grok-build-style step list + waiting chip.

use std::time::{Duration, Instant};

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::theme::{self, Face};
use crate::prof;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Quiet,
    Steps,
    Full,
}

impl Verbosity {
    pub fn next(self) -> Self {
        match self {
            Self::Quiet => Self::Steps,
            Self::Steps => Self::Full,
            Self::Full => Self::Quiet,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Steps => "steps",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    Think,
    Strike,
    Wait,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub kind: StepKind,
    pub title: String,
    pub body: String,
    pub t0: Instant,
    pub dur: Option<Duration>,
    pub ok: Option<bool>,
    pub tokens: u32,
    pub out_lines: u32,
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub steps: Vec<Step>,
    pub started: Instant,
    pub tokens: u32,
    pub phase: String,
}

impl Activity {
    pub fn start() -> Self {
        Self {
            steps: vec![Step {
                kind: StepKind::Wait,
                title: "Waiting".into(),
                body: String::new(),
                t0: Instant::now(),
                dur: None,
                ok: None,
                tokens: 0,
                out_lines: 0,
            }],
            started: Instant::now(),
            tokens: 0,
            phase: "waiting".into(),
        }
    }

    pub fn on_status(&mut self, status: &str) {
        self.phase = status.into();
        if status == "waiting" || status == "thinking" {
            if !matches!(
                self.steps.last().map(|s| s.kind),
                Some(StepKind::Wait | StepKind::Think)
            ) {
                self.steps.push(Step {
                    kind: StepKind::Wait,
                    title: "Waiting".into(),
                    body: String::new(),
                    t0: Instant::now(),
                    dur: None,
                    ok: None,
                    tokens: 0,
                    out_lines: 0,
                });
            } else if let Some(s) = self.steps.last_mut() {
                s.kind = StepKind::Wait;
                s.title = "Waiting".into();
            }
        }
        if status == "striking" {
            self.close_open();
        }
    }

    pub fn on_delta(&mut self, kind: &str, text: &str) {
        self.tokens = self.tokens.saturating_add(prof::estimate_tokens(text));
        self.phase = if kind == "reason" {
            "thinking"
        } else {
            "decode"
        }
        .into();
        let last = self.steps.last_mut();
        match last {
            Some(s) if matches!(s.kind, StepKind::Wait | StepKind::Think) && s.dur.is_none() => {
                s.kind = StepKind::Think;
                s.title = "Thought".into();
                s.body.push_str(text);
                s.tokens = s.tokens.saturating_add(prof::estimate_tokens(text));
            }
            _ => {
                self.steps.push(Step {
                    kind: StepKind::Think,
                    title: "Thought".into(),
                    body: text.into(),
                    t0: Instant::now(),
                    dur: None,
                    ok: None,
                    tokens: prof::estimate_tokens(text),
                    out_lines: 0,
                });
            }
        }
    }

    pub fn close_think(&mut self) {
        if let Some(s) = self.steps.last_mut() {
            if s.dur.is_none() && matches!(s.kind, StepKind::Think | StepKind::Wait) {
                s.dur = Some(s.t0.elapsed());
                if s.kind == StepKind::Think {
                    s.title = format!("Thought for {}", fmt_dur(s.dur.unwrap()));
                }
            }
        }
    }

    pub fn on_strike(&mut self, code: &str, stdout: &str, ok: bool, ms: Option<u64>) {
        self.close_think();
        self.phase = "striking".into();
        let first = code.lines().find(|l| !l.trim().is_empty()).unwrap_or(code);
        let title = clip(first, 48);
        self.steps.push(Step {
            kind: StepKind::Strike,
            title,
            body: code.into(),
            t0: Instant::now() - Duration::from_millis(ms.unwrap_or(0)),
            dur: ms.map(Duration::from_millis),
            ok: Some(ok),
            tokens: 0,
            out_lines: stdout.lines().count() as u32,
        });
    }

    pub fn finish(&mut self) {
        self.close_think();
        self.phase = "idle".into();
    }

    fn close_open(&mut self) {
        if let Some(s) = self.steps.last_mut() {
            if s.dur.is_none() {
                s.dur = Some(s.t0.elapsed());
                if s.kind == StepKind::Think {
                    s.title = format!("Thought for {}", fmt_dur(s.dur.unwrap()));
                }
            }
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn chip(&self) -> String {
        let wait = fmt_dur(self.elapsed());
        let tok = if self.tokens == 0 {
            String::new()
        } else {
            format!(" · ↑ {} tok", fmt_tok(self.tokens))
        };
        match self.phase.as_str() {
            "striking" => {
                let last = self
                    .steps
                    .iter()
                    .rev()
                    .find(|s| s.kind == StepKind::Strike)
                    .map(|s| s.title.as_str())
                    .unwrap_or("strike");
                format!("◆ python · {last} · {wait}")
            }
            "decode" => format!("⋮ decode · {wait}{tok}"),
            "thinking" => format!("⋮ thinking · {wait}{tok}"),
            _ => format!("⋮ Waiting · {wait}{tok}"),
        }
    }
}

pub fn fmt_dur(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s < 1.0 {
        format!("{:.0}ms", d.as_secs_f64() * 1000.0)
    } else if s < 60.0 {
        format!("{s:.1}s")
    } else {
        let m = d.as_secs() / 60;
        let rem = d.as_secs() % 60;
        format!("{m}m {rem:02}s")
    }
}

pub fn fmt_tok(n: u32) -> String {
    if n >= 10_000 {
        format!("{:.0}k", n as f64 / 1000.0)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn clip(text: &str, max: usize) -> String {
    let t = text.trim();
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= max {
        t.to_string()
    } else {
        format!("{}…", chars[..max].iter().collect::<String>())
    }
}

pub fn step_line(step: &Step, verbose: bool) -> Vec<Line<'static>> {
    let th = theme::t();
    let (mark, face) = match (step.kind, step.ok) {
        (StepKind::Strike, Some(false)) => ("◆", Face::StepFail),
        (StepKind::Strike, _) => ("◆", Face::StepRun),
        (StepKind::Think, _) => ("◆", Face::StepThink),
        (StepKind::Wait, _) => ("⋮", Face::StepWait),
    };
    let extra = match step.kind {
        StepKind::Strike => {
            let mut bits = Vec::new();
            if step.out_lines > 0 {
                bits.push(format!("↓ {} lines", step.out_lines));
            }
            if let Some(d) = step.dur {
                bits.push(fmt_dur(d));
            }
            if bits.is_empty() {
                String::new()
            } else {
                format!(" · {}", bits.join(" · "))
            }
        }
        StepKind::Think => {
            if step.title.starts_with("Thought for") {
                String::new()
            } else if let Some(d) = step.dur {
                format!(" for {}", fmt_dur(d))
            } else {
                String::new()
            }
        }
        StepKind::Wait => String::new(),
    };
    let label = match step.kind {
        StepKind::Strike => format!(" python · {}{extra}", step.title),
        StepKind::Think => format!(" {}{extra}", step.title),
        StepKind::Wait => format!(" {}", step.title),
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!(" {mark}"), th.style(face)),
        Span::styled(label, th.style(face)),
    ])];
    if verbose && !step.body.is_empty() && step.kind != StepKind::Wait {
        let preview: Vec<&str> = step.body.lines().take(8).collect();
        for row in preview {
            lines.push(Line::from(Span::styled(
                format!("    {row}"),
                th.style(Face::StepMute),
            )));
        }
    }
    lines
}

pub fn chip_line(text: &str) -> Line<'static> {
    let th = theme::t();
    Line::from(Span::styled(format!(" {text} "), th.style(Face::StepWait)))
}

#[allow(dead_code)]
pub fn style_ok() -> Style {
    theme::t().style(Face::StepRun)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_starts_as_waiting() {
        let act = Activity::start();
        let chip = act.chip();
        assert!(chip.contains("Waiting"), "{chip}");
    }

    #[test]
    fn deltas_become_thought_then_strike() {
        let mut act = Activity::start();
        act.on_delta("reason", "plan");
        act.on_delta("content", "print(1)");
        act.close_think();
        act.on_strike("print(1)\n", "1\n", true, Some(7));
        act.finish();
        assert!(act.steps.iter().any(|s| s.title.contains("Thought")));
        let strike = act
            .steps
            .iter()
            .find(|s| s.kind == StepKind::Strike)
            .unwrap();
        assert!(strike.title.contains("print(1)"));
        assert_eq!(strike.out_lines, 1);
        assert_eq!(strike.ok, Some(true));
    }

    #[test]
    fn verbosity_cycles() {
        assert_eq!(Verbosity::Quiet.next(), Verbosity::Steps);
        assert_eq!(Verbosity::Steps.next(), Verbosity::Full);
        assert_eq!(Verbosity::Full.next(), Verbosity::Quiet);
    }

    #[test]
    fn fmt_tok_compacts() {
        assert_eq!(fmt_tok(28_000), "28k");
        assert_eq!(fmt_tok(280), "280");
    }
}
