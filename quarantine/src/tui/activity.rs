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
    Prefill,
    Think,
    Decode,
    Tool,
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

impl StepKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prefill => "Prefill",
            Self::Think => "Think",
            Self::Decode => "Decode",
            Self::Tool => "Tool",
        }
    }

    pub fn from_delta(kind: &str) -> Self {
        if kind == "reason" {
            Self::Think
        } else {
            Self::Decode
        }
    }

    pub fn from_phase_name(name: Option<&str>) -> Self {
        match name {
            Some("prefill") => Self::Prefill,
            Some("decode") => Self::Decode,
            Some("tool") => Self::Tool,
            _ => Self::Think,
        }
    }

    pub fn as_phase_name(self) -> &'static str {
        match self {
            Self::Prefill => "prefill",
            Self::Think => "think",
            Self::Decode => "decode",
            Self::Tool => "tool",
        }
    }

    pub fn face(self, ok: Option<bool>) -> Face {
        match (self, ok) {
            (Self::Tool, Some(false)) => Face::StepFail,
            (Self::Prefill, _) => Face::PlotPrefill,
            (Self::Think, _) => Face::PlotThink,
            (Self::Decode, _) => Face::PlotDecode,
            (Self::Tool, _) => Face::PlotTool,
        }
    }
}

impl Activity {
    pub fn start() -> Self {
        Self {
            steps: vec![Step {
                kind: StepKind::Prefill,
                title: StepKind::Prefill.label().into(),
                body: String::new(),
                t0: Instant::now(),
                dur: None,
                ok: None,
                tokens: 0,
                out_lines: 0,
            }],
            started: Instant::now(),
            tokens: 0,
            phase: "prefill".into(),
        }
    }

    pub fn on_status(&mut self, status: &str) -> Option<Step> {
        self.phase = status.into();
        if status == "waiting" {
            if !matches!(
                self.steps.last().map(|s| (s.kind, s.dur.is_none())),
                Some((StepKind::Prefill, true))
            ) {
                let closed = self.close_open_take();
                self.steps.push(open_step(StepKind::Prefill, ""));
                return closed;
            }
        }
        if status == "striking" {
            return self.close_open_take();
        }
        None
    }

    /// Apply a stream delta. Returns a step that just closed.
    pub fn on_delta(&mut self, kind: &str, text: &str) -> Option<Step> {
        let next = StepKind::from_delta(kind);
        let add = prof::estimate_tokens(text);
        self.tokens = self.tokens.saturating_add(add);
        self.phase = next.as_phase_name().into();
        if let Some(s) = self.steps.last_mut() {
            if s.kind == next && s.dur.is_none() {
                s.body.push_str(text);
                s.tokens = s.tokens.saturating_add(add);
                s.title = live_title(s);
                return None;
            }
        }
        let closed = self.close_open_take();
        self.steps.push(open_step(next, text));
        closed
    }

    pub fn on_strike(&mut self, code: &str, stdout: &str, ok: bool, ms: Option<u64>) -> Option<Step> {
        let closed = self.close_open_take();
        self.phase = "tool".into();
        let first = code.lines().find(|l| !l.trim().is_empty()).unwrap_or(code);
        let title = clip(first, 48);
        self.steps.push(Step {
            kind: StepKind::Tool,
            title,
            body: code.into(),
            t0: Instant::now() - Duration::from_millis(ms.unwrap_or(0)),
            dur: ms.map(Duration::from_millis),
            ok: Some(ok),
            tokens: 0,
            out_lines: stdout.lines().count() as u32,
        });
        closed
    }

    pub fn finish(&mut self) -> Option<Step> {
        let closed = self.close_open_take();
        self.phase = "idle".into();
        closed
    }

    pub fn close_open_take(&mut self) -> Option<Step> {
        let s = self.steps.last_mut()?;
        if s.dur.is_some() {
            return None;
        }
        s.dur = Some(s.t0.elapsed());
        s.title = live_title(s);
        Some(s.clone())
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
            "tool" | "striking" => {
                let last = self
                    .steps
                    .iter()
                    .rev()
                    .find(|s| s.kind == StepKind::Tool)
                    .map(|s| s.title.as_str())
                    .unwrap_or("strike");
                format!("→ Tool · {last} · {wait}")
            }
            "decode" => format!("⋮ Decode · {wait}{tok}"),
            "think" | "thinking" => format!("⋮ Think · {wait}{tok}"),
            _ => format!("⋮ Prefill · {wait}{tok}"),
        }
    }
}

fn open_step(kind: StepKind, text: &str) -> Step {
    let tokens = prof::estimate_tokens(text);
    let mut step = Step {
        kind,
        title: kind.label().into(),
        body: text.into(),
        t0: Instant::now(),
        dur: None,
        ok: None,
        tokens,
        out_lines: 0,
    };
    step.title = live_title(&step);
    step
}

fn live_title(step: &Step) -> String {
    let mut bits = vec![step.kind.label().to_string()];
    if let Some(d) = step.dur {
        bits.push(fmt_dur(d));
    } else if matches!(step.kind, StepKind::Think | StepKind::Decode | StepKind::Prefill) {
        bits.push("streaming".into());
    }
    if step.tokens > 0 {
        bits.push(format!("{} tok", fmt_tok(step.tokens)));
    }
    if step.kind == StepKind::Tool && !step.title.is_empty() && step.title != step.kind.label() {
        // tool title is the first line of code; keep it
    }
    if step.kind == StepKind::Tool {
        return format!("{} · {}", step.kind.label(), step.title);
    }
    bits.join(" · ")
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
    let face = step.kind.face(step.ok);
    let mark = match step.kind {
        StepKind::Prefill => "·",
        StepKind::Think => "+",
        StepKind::Decode => "·",
        StepKind::Tool => "→",
    };
    let mut extra = Vec::new();
    if step.kind == StepKind::Tool {
        if step.out_lines > 0 {
            extra.push(format!("↓ {} lines", step.out_lines));
        }
        extra.push(fmt_dur(step.dur.unwrap_or_default()));
    }
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!(" · {}", extra.join(" · "))
    };
    let label = match step.kind {
        StepKind::Tool => format!(" {} · {}{extra}", step.kind.label(), step.title),
        _ => format!(" {}{extra}", step.title),
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!(" {mark}"), th.style(face)),
        Span::styled(label, th.style(face)),
    ])];
    if verbose && !step.body.is_empty() && step.kind != StepKind::Prefill {
        const CAP: usize = 400;
        let body: Vec<&str> = step.body.lines().collect();
        let more = body.len().saturating_sub(CAP);
        for row in body.into_iter().take(CAP) {
            lines.push(Line::from(Span::styled(
                format!("    {row}"),
                th.style(Face::StepMute),
            )));
        }
        if more > 0 {
            lines.push(Line::from(Span::styled(
                format!("    … {more} more lines"),
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
        assert!(chip.contains("Prefill"), "{chip}");
    }

    #[test]
    fn deltas_become_thought_then_strike() {
        let mut act = Activity::start();
        let prefill = act.on_delta("reason", "plan");
        assert!(prefill.is_some_and(|s| s.kind == StepKind::Prefill));
        let think = act.on_delta("content", "print(1)");
        assert!(think.is_some_and(|s| s.kind == StepKind::Think && s.body.contains("plan")));
        let decode = act.on_strike("print(1)\n", "1\n", true, Some(7));
        assert!(decode.is_some_and(|s| s.kind == StepKind::Decode));
        act.finish();
        assert!(act.steps.iter().any(|s| s.kind == StepKind::Think));
        assert!(act.steps.iter().any(|s| s.kind == StepKind::Decode));
        let strike = act
            .steps
            .iter()
            .find(|s| s.kind == StepKind::Tool)
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
