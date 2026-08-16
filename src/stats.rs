//! DSH-shaped folds over the event log. The log is the product;
//! these are the standing figures the viz half will read.

use crate::frame::{Event, EventBody};
use crate::prof::{self, Timing};

/// Whole-log conversation figures. Paging must not change them.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStats {
    pub turns: u32,
    pub steps: u32,
    pub llm_ns: u64,
    pub ttft_ns: u64,
    pub ttft_steps: u32,
    pub decode_ns: u64,
    pub decode_tokens: u32,
    pub tool_ns: u64,
}

impl SessionStats {
    pub fn ttft_avg_ns(&self) -> Option<u64> {
        if self.ttft_steps == 0 {
            None
        } else {
            Some(self.ttft_ns / u64::from(self.ttft_steps))
        }
    }

    pub fn tok_s(&self) -> Option<f64> {
        if self.decode_tokens == 0 || self.decode_ns == 0 {
            None
        } else {
            Some(self.decode_tokens as f64 / (self.decode_ns as f64 / 1_000_000_000.0))
        }
    }

    pub fn compact(&self) -> String {
        let mut parts = vec![
            format!("{} turn", self.turns),
            format!("{} step", self.steps),
        ];
        if self.llm_ns > 0 {
            parts.push(format!("llm {}", prof::fmt_ns(self.llm_ns)));
        }
        if let Some(avg) = self.ttft_avg_ns() {
            parts.push(format!("ttft avg {}", prof::fmt_ns(avg)));
        }
        if let Some(s) = self.tok_s() {
            parts.push(format!("{s:.1} tok/s"));
        }
        if self.tool_ns > 0 {
            parts.push(format!("tool {}", prof::fmt_ns(self.tool_ns)));
        }
        parts.join("  ")
    }

    fn ingest_step(&mut self, t: &Timing) {
        self.steps += 1;
        self.llm_ns = self.llm_ns.saturating_add(llm_ns(t));
        if let Some(n) = t.ttft_ns.or(t.prefill_ns) {
            self.ttft_ns = self.ttft_ns.saturating_add(n);
            self.ttft_steps += 1;
        }
        if let Some(n) = t.decode_ns {
            self.decode_ns = self.decode_ns.saturating_add(n);
        }
        let out = t.decode_tokens();
        if out > 0 {
            self.decode_tokens = self.decode_tokens.saturating_add(out);
        }
    }
}

fn llm_ns(t: &Timing) -> u64 {
    let model = t
        .prefill_ns
        .unwrap_or(0)
        .saturating_add(t.decode_ns.unwrap_or(0))
        .saturating_add(t.reason_ns.unwrap_or(0));
    if model > 0 {
        model
    } else {
        t.wall_ns.saturating_sub(t.strike_ns.unwrap_or(0))
    }
}

/// Disjoint billed buckets. Reasoning is a subdivision of output.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TokenUsage {
    pub uncached_in: u32,
    pub cache_read: u32,
    pub cache_write: u32,
    pub output: u32,
    pub reason: u32,
}

impl TokenUsage {
    pub fn billed_in(&self) -> u32 {
        self.uncached_in
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    pub fn cache_hit(&self) -> Option<f64> {
        let t = self.billed_in();
        if t == 0 {
            None
        } else {
            Some(f64::from(self.cache_read) / f64::from(t))
        }
    }

    pub fn add_timing(&mut self, t: &Timing) {
        self.uncached_in = self.uncached_in.saturating_add(t.tokens_in.unwrap_or(0));
        self.cache_read = self
            .cache_read
            .saturating_add(t.tokens_cache_read.unwrap_or(0));
        self.cache_write = self
            .cache_write
            .saturating_add(t.tokens_cache_write.unwrap_or(0));
        self.output = self.output.saturating_add(t.tokens_out.unwrap_or(0));
        self.reason = self.reason.saturating_add(t.tokens_reason.unwrap_or(0));
    }

    pub fn compact(&self) -> String {
        let mut parts = vec![format!("in {}", self.billed_in())];
        if self.cache_read > 0 || self.cache_write > 0 {
            parts.push(format!("cache {}r/{}w", self.cache_read, self.cache_write));
        }
        if let Some(h) = self.cache_hit() {
            parts.push(format!("hit {:.0}%", h * 100.0));
        }
        parts.push(format!("out {}", self.output));
        if self.reason > 0 {
            parts.push(format!("think {}", self.reason));
        }
        parts.join("  ")
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContextView {
    pub pressure: Option<u32>,
    pub projected: Option<u32>,
    pub window: Option<u32>,
    pub system: u32,
    pub tools: u32,
    pub messages: u32,
}

impl ContextView {
    pub fn compact(&self) -> String {
        let mut parts = Vec::new();
        if let Some(p) = self.projected.or(self.pressure) {
            match self.window {
                Some(w) if w > 0 => parts.push(format!(
                    "~{p}/{w} ({:.0}%)",
                    100.0 * f64::from(p) / f64::from(w)
                )),
                _ => parts.push(format!("~{p}")),
            }
        }
        parts.push(format!(
            "sys {} · tools {} · msg {}",
            self.system, self.tools, self.messages
        ));
        parts.join("  ")
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fold {
    pub stats: SessionStats,
    pub usage: TokenUsage,
    pub context: ContextView,
}

/// One discrete lifecycle span. Agent work is phases, not ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Prefill,
    Think,
    Decode,
    Tool,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prefill => "Prefill",
            Self::Think => "Think",
            Self::Decode => "Decode",
            Self::Tool => "Tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraceRow {
    pub id: u32,
    pub turn: u32,
    pub phase: Phase,
    pub dur_ns: u64,
    pub tokens: Option<u32>,
    pub tok_s: Option<f64>,
    pub cache_hit: Option<f64>,
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trace {
    pub rows: Vec<TraceRow>,
    pub last_turn: Vec<TraceRow>,
    pub ttfts: Vec<u64>,
    pub thinks: Vec<u64>,
    pub decodes: Vec<u64>,
    pub tok_s: Vec<f64>,
}

impl Trace {
    pub fn last_of(&self, phase: Phase) -> Option<&TraceRow> {
        self.last_turn
            .iter()
            .rev()
            .find(|r| r.phase == phase)
            .or_else(|| self.rows.iter().rev().find(|r| r.phase == phase))
    }

    pub fn last_ttft(&self) -> Option<u64> {
        self.last_of(Phase::Prefill).map(|r| r.dur_ns)
    }

    pub fn last_think(&self) -> Option<u64> {
        self.last_of(Phase::Think).map(|r| r.dur_ns)
    }

    pub fn last_decode(&self) -> Option<u64> {
        self.last_of(Phase::Decode).map(|r| r.dur_ns)
    }

    pub fn last_tok_s(&self) -> Option<f64> {
        self.last_of(Phase::Decode)
            .and_then(|r| r.tok_s)
            .or_else(|| self.tok_s.last().copied())
    }

    pub fn last_strike(&self) -> Option<u64> {
        self.last_of(Phase::Tool).map(|r| r.dur_ns)
    }
}

pub fn percentile(xs: &[u64], p: f64) -> Option<u64> {
    if xs.is_empty() {
        return None;
    }
    let mut v = xs.to_vec();
    v.sort_unstable();
    let rank = (p.clamp(0.0, 100.0) / 100.0) * (v.len().saturating_sub(1) as f64);
    Some(v[rank.round() as usize])
}

pub fn fmt_tokens(n: u32) -> String {
    if n >= 100_000 {
        format!("{:.0}k", f64::from(n) / 1000.0)
    } else if n >= 1000 {
        format!("{:.1}k", f64::from(n) / 1000.0)
    } else {
        n.to_string()
    }
}

/// Project the log into ordered lifecycle spans. One Step becomes
/// Prefill / Think / Decode; a Strike is Tool. Answer timing is
/// ignored when a Step already closed the model work.
pub fn trace(events: &[Event]) -> Trace {
    let mut out = Trace::default();
    let mut turn: u32 = 0;
    let mut in_turn = false;
    let mut turn_start = 0usize;
    let mut saw_step = false;
    let mut next_id = 1u32;

    for ev in events {
        match &ev.body {
            EventBody::Ask { .. } => {
                close_turn(&mut out, turn_start);
                turn = turn.saturating_add(1);
                in_turn = true;
                saw_step = false;
                turn_start = out.rows.len();
            }
            EventBody::Step { timing, .. } => {
                if !in_turn {
                    turn = turn.saturating_add(1);
                    in_turn = true;
                    turn_start = out.rows.len();
                }
                push_timing(&mut out, &mut next_id, turn, timing);
                saw_step = true;
            }
            EventBody::Strike {
                code,
                error,
                ok,
                ms,
                timing,
                ..
            } => {
                if !in_turn {
                    turn = turn.saturating_add(1);
                    in_turn = true;
                    turn_start = out.rows.len();
                }
                let dur = timing
                    .as_ref()
                    .and_then(|t| t.strike_ns.or(Some(t.wall_ns)))
                    .or_else(|| ms.map(|m| m.saturating_mul(1_000_000)))
                    .unwrap_or(0);
                let tool = code
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or(code)
                    .trim();
                let tool = clip_status(tool, 36);
                let status = if !*ok {
                    error
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("FAIL {s}"))
                        .unwrap_or_else(|| "FAIL".into())
                } else if tool.is_empty() {
                    "ok".into()
                } else {
                    format!("tool: {tool}")
                };
                out.rows.push(TraceRow {
                    id: next_id,
                    turn,
                    phase: Phase::Tool,
                    dur_ns: dur,
                    tokens: None,
                    tok_s: None,
                    cache_hit: None,
                    status,
                });
                next_id = next_id.saturating_add(1);
            }
            EventBody::Answer { timing, .. } => {
                if !saw_step {
                    if let Some(t) = timing.as_ref().filter(|t| !t.is_empty()) {
                        if !in_turn {
                            turn = turn.saturating_add(1);
                            turn_start = out.rows.len();
                        }
                        push_timing(&mut out, &mut next_id, turn, t);
                    }
                }
                close_turn(&mut out, turn_start);
                in_turn = false;
                saw_step = false;
                turn_start = out.rows.len();
            }
            _ => {}
        }
    }
    if in_turn {
        close_turn(&mut out, turn_start);
    } else if out.last_turn.is_empty() {
        close_turn(&mut out, turn_start);
    }
    out
}

fn close_turn(out: &mut Trace, start: usize) {
    if start < out.rows.len() {
        out.last_turn = out.rows[start..].to_vec();
    }
}

fn push_timing(out: &mut Trace, next_id: &mut u32, turn: u32, t: &Timing) {
    let hit = cache_hit(t);
    if let Some(n) = t.ttft_ns.or(t.prefill_ns).filter(|n| *n > 0) {
        let status = match hit {
            Some(h) => format!("HIT (Cache {:.0}%)", h * 100.0),
            None => "ok".into(),
        };
        out.rows.push(TraceRow {
            id: *next_id,
            turn,
            phase: Phase::Prefill,
            dur_ns: n,
            tokens: t.tokens_in.or(t.tokens_cache_read),
            tok_s: None,
            cache_hit: hit,
            status,
        });
        *next_id = next_id.saturating_add(1);
        out.ttfts.push(n);
    }
    if let Some(n) = t.reason_ns.filter(|n| *n > 0) {
        let tok_s = match t.tokens_reason {
            Some(toks) if toks > 0 => Some(toks as f64 / (n as f64 / 1_000_000_000.0)),
            _ => None,
        };
        out.rows.push(TraceRow {
            id: *next_id,
            turn,
            phase: Phase::Think,
            dur_ns: n,
            tokens: t.tokens_reason,
            tok_s,
            cache_hit: None,
            status: "COMPLETED".into(),
        });
        *next_id = next_id.saturating_add(1);
        out.thinks.push(n);
    }
    if let Some(n) = t.decode_ns.filter(|n| *n > 0) {
        let decode_toks = t.decode_tokens();
        let mut tmp = t.clone();
        tmp.recompute_tok_s();
        let tok_s = tmp.tok_s;
        out.rows.push(TraceRow {
            id: *next_id,
            turn,
            phase: Phase::Decode,
            dur_ns: n,
            tokens: Some(decode_toks).filter(|n| *n > 0),
            tok_s,
            cache_hit: None,
            status: "COMPLETED".into(),
        });
        *next_id = next_id.saturating_add(1);
        out.decodes.push(n);
        if let Some(s) = tok_s {
            out.tok_s.push(s);
        }
    } else if t.ttft_ns.or(t.prefill_ns).is_none()
        && t.reason_ns.is_none()
        && t.wall_ns > t.strike_ns.unwrap_or(0)
    {
        let n = t.wall_ns.saturating_sub(t.strike_ns.unwrap_or(0));
        out.rows.push(TraceRow {
            id: *next_id,
            turn,
            phase: Phase::Decode,
            dur_ns: n,
            tokens: t.tokens_out,
            tok_s: t.tok_s,
            cache_hit: None,
            status: "COMPLETED".into(),
        });
        *next_id = next_id.saturating_add(1);
        if n > 0 {
            out.decodes.push(n);
        }
        if let Some(s) = t.tok_s {
            out.tok_s.push(s);
        }
    }
}

fn cache_hit(t: &Timing) -> Option<f64> {
    let billed = t.billed_in();
    if billed == 0 {
        None
    } else {
        Some(f64::from(t.tokens_cache_read.unwrap_or(0)) / f64::from(billed))
    }
}

fn clip_status(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        text.to_string()
    } else {
        format!("{}…", chars[..max].iter().collect::<String>())
    }
}

/// Fold the whole log. `system` is the standing system prompt (not logged).
pub fn fold(events: &[Event], system: &str) -> Fold {
    let mut stats = SessionStats::default();
    let mut usage = TokenUsage::default();
    let mut open_ask = false;
    let mut saw_step = false;
    let mut last_billed: Option<(u64, u32)> = None;

    for ev in events {
        match &ev.body {
            EventBody::Ask { timing, .. } => {
                open_ask = true;
                saw_step = false;
                if let Some(t) = timing {
                    usage.add_timing(t);
                    if t.billed_in() > 0 {
                        last_billed = Some((ev.seq, t.billed_in()));
                    }
                }
            }
            EventBody::Step { timing, .. } => {
                stats.ingest_step(timing);
                usage.add_timing(timing);
                saw_step = true;
                if timing.billed_in() > 0 {
                    last_billed = Some((ev.seq, timing.billed_in()));
                }
            }
            EventBody::Strike { timing, ms, .. } => {
                let ns = timing
                    .as_ref()
                    .and_then(|t| t.strike_ns.or(Some(t.wall_ns)))
                    .or_else(|| ms.map(|m| m.saturating_mul(1_000_000)))
                    .unwrap_or(0);
                stats.tool_ns = stats.tool_ns.saturating_add(ns);
            }
            EventBody::Answer { timing, .. } => {
                if !saw_step {
                    if let Some(t) = timing.as_ref().filter(|t| !t.is_empty()) {
                        stats.ingest_step(t);
                        usage.add_timing(t);
                        if t.billed_in() > 0 {
                            last_billed = Some((ev.seq, t.billed_in()));
                        }
                    } else if open_ask {
                        stats.steps += 1;
                    }
                }
                if open_ask {
                    stats.turns += 1;
                }
                open_ask = false;
                saw_step = false;
            }
            _ => {}
        }
    }

    let mut visible = String::new();
    let mut after_last = String::new();
    let cut = last_billed.map(|(seq, _)| seq);
    for ev in events {
        let Some(text) = visible_text(&ev.body) else {
            continue;
        };
        visible.push_str(text);
        visible.push('\n');
        if cut.is_some_and(|s| ev.seq > s) {
            after_last.push_str(text);
            after_last.push('\n');
        }
    }
    let system_tok = prof::estimate_tokens(system);
    let msg_tok = prof::estimate_tokens(&visible);
    let pressure = last_billed.map(|(_, n)| n);
    let projected = pressure.map(|p| p.saturating_add(prof::estimate_tokens(&after_last)));
    Fold {
        stats,
        usage,
        context: ContextView {
            pressure,
            projected,
            window: None,
            system: system_tok,
            tools: 0,
            messages: msg_tok,
        },
    }
}

/// Fold every session that has a log.
pub fn fold_sessions(
    root: &crate::frame::FrameRoot,
    system: &str,
) -> std::collections::BTreeMap<String, Fold> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(sessions) = root.list_sessions() else {
        return out;
    };
    for s in sessions {
        let Ok(ev) = root.load_events(&s.id) else {
            continue;
        };
        if ev.is_empty() {
            continue;
        }
        out.insert(s.id, fold(&ev, system));
    }
    out
}

/// Prefer the front member if it is a session with a log; else the busiest.
pub fn pick_session(
    front: Option<&str>,
    folds: &std::collections::BTreeMap<String, Fold>,
) -> Option<String> {
    if let Some(id) = front {
        if folds.contains_key(id) {
            return Some(id.to_string());
        }
    }
    folds
        .iter()
        .max_by_key(|(id, f)| (f.stats.steps, f.stats.turns, f.usage.output, id.as_str()))
        .map(|(id, _)| id.clone())
}

fn visible_text(body: &EventBody) -> Option<&str> {
    match body {
        EventBody::User { text } | EventBody::Ask { prompt: text, .. } => Some(text),
        EventBody::Answer { text, .. } => Some(text),
        EventBody::Strike { code, stdout, .. } => {
            if stdout.is_empty() {
                Some(code)
            } else {
                Some(stdout)
            }
        }
        EventBody::See { text, .. } => Some(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(seq: u64, body: EventBody) -> Event {
        Event {
            seq,
            ts: seq * 10,
            body,
        }
    }

    fn step(ttft: u64, decode: u64, out: u32, uncached: u32) -> Timing {
        let mut t = Timing {
            wall_ns: ttft + decode,
            prefill_ns: Some(ttft),
            ttft_ns: Some(ttft),
            decode_ns: Some(decode),
            tokens_in: Some(uncached),
            tokens_out: Some(out),
            ..Timing::default()
        };
        t.recompute_tok_s();
        t
    }

    #[test]
    fn sums_ttft_and_counts_steps() {
        let events = vec![
            ev(
                0,
                EventBody::Ask {
                    prompt: "hi".into(),
                    provider: None,
                    model: None,
                    timing: None,
                },
            ),
            ev(
                1,
                EventBody::Step {
                    n: 1,
                    timing: step(800_000_000, 5_000_000_000, 200, 100),
                },
            ),
            ev(
                2,
                EventBody::Strike {
                    code: "1".into(),
                    stdout: "1".into(),
                    stderr: String::new(),
                    error: None,
                    ok: true,
                    ms: Some(7),
                    timing: Some(Timing {
                        wall_ns: 7_000_000,
                        strike_ns: Some(7_000_000),
                        ..Timing::default()
                    }),
                },
            ),
            ev(
                3,
                EventBody::Answer {
                    text: "1".into(),
                    timing: Some(step(800_000_000, 5_000_000_000, 200, 100)),
                },
            ),
        ];
        let fold = fold(&events, "sys");
        assert_eq!(fold.stats.turns, 1);
        assert_eq!(fold.stats.steps, 1);
        assert_eq!(fold.stats.ttft_ns, 800_000_000);
        assert_eq!(fold.stats.ttft_steps, 1);
        assert_eq!(fold.stats.decode_tokens, 200);
        assert_eq!(fold.stats.tool_ns, 7_000_000);
        assert_eq!(fold.usage.uncached_in, 100);
        assert_eq!(fold.usage.output, 200);
    }

    #[test]
    fn legacy_answer_without_step_still_counts() {
        let events = vec![
            ev(
                0,
                EventBody::Ask {
                    prompt: "x".into(),
                    provider: None,
                    model: None,
                    timing: None,
                },
            ),
            ev(
                1,
                EventBody::Answer {
                    text: "y".into(),
                    timing: Some(step(100_000_000, 400_000_000, 20, 10)),
                },
            ),
        ];
        let fold = fold(&events, "");
        assert_eq!(fold.stats.turns, 1);
        assert_eq!(fold.stats.steps, 1);
        assert_eq!(fold.stats.ttft_avg_ns(), Some(100_000_000));
    }

    #[test]
    fn answer_without_timing_still_closes_a_turn() {
        let events = vec![
            ev(
                0,
                EventBody::Ask {
                    prompt: "x".into(),
                    provider: None,
                    model: None,
                    timing: None,
                },
            ),
            ev(
                1,
                EventBody::Answer {
                    text: "y".into(),
                    timing: None,
                },
            ),
        ];
        let fold = fold(&events, "");
        assert_eq!(fold.stats.turns, 1);
        assert_eq!(fold.stats.steps, 1);
        assert_eq!(fold.stats.ttft_steps, 0);
    }

    #[test]
    fn open_ask_without_answer_is_not_a_turn() {
        let events = vec![ev(
            0,
            EventBody::Ask {
                prompt: "x".into(),
                provider: None,
                model: None,
                timing: None,
            },
        )];
        let fold = fold(&events, "");
        assert_eq!(fold.stats.turns, 0);
        assert_eq!(fold.stats.steps, 0);
    }

    #[test]
    fn cache_hit_is_read_over_billed() {
        let mut u = TokenUsage {
            uncached_in: 20,
            cache_read: 80,
            cache_write: 0,
            output: 5,
            reason: 0,
        };
        assert_eq!(u.billed_in(), 100);
        assert!((u.cache_hit().unwrap() - 0.8).abs() < 0.001);
        let t = Timing {
            tokens_in: Some(5),
            tokens_cache_read: Some(10),
            tokens_out: Some(2),
            ..Timing::default()
        };
        u.add_timing(&t);
        assert_eq!(u.uncached_in, 25);
        assert_eq!(u.cache_read, 90);
        assert_eq!(u.output, 7);
    }

    #[test]
    fn pick_session_skips_a_front_pty() {
        let mut folds = std::collections::BTreeMap::new();
        folds.insert(
            "default".into(),
            Fold {
                stats: SessionStats {
                    turns: 2,
                    steps: 2,
                    ..SessionStats::default()
                },
                ..Fold::default()
            },
        );
        assert_eq!(
            pick_session(Some("bash"), &folds).as_deref(),
            Some("default")
        );
        assert_eq!(
            pick_session(Some("default"), &folds).as_deref(),
            Some("default")
        );
    }

    #[test]
    fn trace_splits_a_step_into_phases() {
        let events = vec![
            ev(
                0,
                EventBody::Ask {
                    prompt: "hi".into(),
                    provider: None,
                    model: None,
                    timing: None,
                },
            ),
            ev(
                1,
                EventBody::Step {
                    n: 1,
                    timing: Timing {
                        reason_ns: Some(2_000_000_000),
                        tokens_reason: Some(40),
                        tokens_cache_read: Some(90),
                        tokens_in: Some(10),
                        ..step(400_000_000, 400_000_000, 80, 10)
                    },
                },
            ),
            ev(
                2,
                EventBody::Strike {
                    code: "web_search(\"rust\")".into(),
                    stdout: String::new(),
                    stderr: String::new(),
                    error: None,
                    ok: true,
                    ms: Some(1120),
                    timing: Some(Timing {
                        wall_ns: 1_120_000_000,
                        strike_ns: Some(1_120_000_000),
                        ..Timing::default()
                    }),
                },
            ),
            ev(
                3,
                EventBody::Answer {
                    text: "ok".into(),
                    timing: Some(step(400_000_000, 400_000_000, 80, 10)),
                },
            ),
        ];
        let t = trace(&events);
        assert_eq!(t.last_turn.len(), 4);
        assert_eq!(t.last_turn[0].phase, Phase::Prefill);
        assert_eq!(t.last_turn[1].phase, Phase::Think);
        assert_eq!(t.last_turn[2].phase, Phase::Decode);
        assert_eq!(t.last_turn[3].phase, Phase::Tool);
        assert_eq!(t.last_ttft(), Some(400_000_000));
        let rate = t.last_tok_s().unwrap();
        assert!((rate - 100.0).abs() < 1.0, "{rate}");
        assert!(t.last_turn[0].status.contains("HIT"));
        assert!(t.last_turn[3].status.contains("web_search"));
        assert_eq!(t.rows.len(), 4, "answer must not duplicate the step");
    }

    #[test]
    fn percentile_picks_p95() {
        let xs: Vec<u64> = (1..=20).collect();
        assert_eq!(percentile(&xs, 0.0), Some(1));
        assert_eq!(percentile(&xs, 100.0), Some(20));
        assert_eq!(percentile(&[], 95.0), None);
        let p50 = percentile(&xs, 50.0).unwrap();
        assert!((10..=11).contains(&p50), "{p50}");
        let p95 = percentile(&xs, 95.0).unwrap();
        assert!((19..=20).contains(&p95), "{p95}");
    }

    #[test]
    fn fmt_tokens_compacts() {
        assert_eq!(fmt_tokens(42), "42");
        assert_eq!(fmt_tokens(14200), "14.2k");
    }

    #[test]
    fn two_steps_sum_ttft_lowest_is_not_used() {
        let events = vec![
            ev(
                0,
                EventBody::Ask {
                    prompt: "a".into(),
                    provider: None,
                    model: None,
                    timing: None,
                },
            ),
            ev(
                1,
                EventBody::Step {
                    n: 1,
                    timing: step(1_200_000_000, 1_000_000_000, 10, 8),
                },
            ),
            ev(
                2,
                EventBody::Step {
                    n: 2,
                    timing: step(400_000_000, 1_000_000_000, 10, 8),
                },
            ),
            ev(
                3,
                EventBody::Answer {
                    text: "z".into(),
                    timing: None,
                },
            ),
        ];
        let fold = fold(&events, "");
        assert_eq!(fold.stats.steps, 2);
        assert_eq!(fold.stats.ttft_ns, 1_600_000_000);
        assert_eq!(fold.stats.ttft_steps, 2);
        assert_eq!(fold.stats.ttft_avg_ns(), Some(800_000_000));
        assert_eq!(fold.stats.turns, 1);
    }
}
