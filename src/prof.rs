//! Process-local profiler. The event log is the product; this is the
//! live ring + counters every fiber, serve op, model turn, and TUI
//! frame reports to. Nanoseconds. One instrumentation.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};

const RING: usize = 2048;

static EPOCH: OnceLock<Instant> = OnceLock::new();
static PROF: OnceLock<Mutex<Inner>> = OnceLock::new();

fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

fn inner() -> &'static Mutex<Inner> {
    PROF.get_or_init(|| Mutex::new(Inner::new()))
}

/// Monotonic ns since process start.
pub fn now_ns() -> u64 {
    epoch().elapsed().as_nanos() as u64
}

pub fn ns(d: std::time::Duration) -> u64 {
    d.as_nanos() as u64
}

struct Inner {
    samples: VecDeque<Sample>,
    counters: BTreeMap<String, u64>,
    last_model: Option<Timing>,
}

impl Inner {
    fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(RING),
            counters: BTreeMap::new(),
            last_model: None,
        }
    }

    fn push(&mut self, sample: Sample) {
        if self.samples.len() == RING {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }
}

/// One closed span on the ring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub name: String,
    pub group: String,
    pub t0_ns: u64,
    pub dur_ns: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<String>,
}

/// Folded model/tool turn. Lives on Ask/Strike/Answer events.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wall_ns: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_in: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_out: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tok_s: Option<f64>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Timing {
    pub fn wall(d: std::time::Duration) -> Self {
        Self {
            wall_ns: ns(d),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.wall_ns == 0
            && self.prefill_ns.is_none()
            && self.ttft_ns.is_none()
            && self.decode_ns.is_none()
            && self.reason_ns.is_none()
            && self.strike_ns.is_none()
            && self.tokens_out.is_none()
    }

    pub fn add_strike(&mut self, strike_ns: u64) {
        self.strike_ns = Some(self.strike_ns.unwrap_or(0).saturating_add(strike_ns));
    }

    pub fn merge_model(&mut self, other: &Timing) {
        self.wall_ns = self.wall_ns.saturating_add(other.wall_ns);
        self.prefill_ns = sum_opt(self.prefill_ns, other.prefill_ns);
        self.ttft_ns = match (self.ttft_ns, other.ttft_ns) {
            (None, x) | (x, None) => x,
            (Some(a), Some(b)) => Some(a.min(b)),
        };
        self.decode_ns = sum_opt(self.decode_ns, other.decode_ns);
        self.reason_ns = sum_opt(self.reason_ns, other.reason_ns);
        self.tokens_in = sum_opt_u32(self.tokens_in, other.tokens_in);
        self.tokens_out = sum_opt_u32(self.tokens_out, other.tokens_out);
        self.recompute_tok_s();
    }

    pub fn recompute_tok_s(&mut self) {
        let toks = self.tokens_out.unwrap_or(0);
        let dec = self.decode_ns.unwrap_or(0);
        self.tok_s = if toks > 0 && dec > 0 {
            Some(toks as f64 / (dec as f64 / 1_000_000_000.0))
        } else {
            None
        };
    }

    pub fn compact(&self) -> String {
        let mut parts = Vec::new();
        if let Some(n) = self.ttft_ns.or(self.prefill_ns) {
            parts.push(format!("ttft {}", fmt_ns(n)));
        }
        if let Some(n) = self.reason_ns {
            parts.push(format!("think {}", fmt_ns(n)));
        }
        if let Some(n) = self.decode_ns {
            parts.push(format!("dec {}", fmt_ns(n)));
        }
        if let Some(n) = self.tokens_out {
            parts.push(format!("{n} tok"));
        }
        if let Some(s) = self.tok_s {
            parts.push(format!("{s:.1} tok/s"));
        }
        if let Some(n) = self.strike_ns {
            parts.push(format!("strike {}", fmt_ns(n)));
        }
        if parts.is_empty() && self.wall_ns > 0 {
            parts.push(fmt_ns(self.wall_ns));
        }
        parts.join("  ")
    }
}

fn sum_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(x), Some(y)) => Some(x.saturating_add(y)),
    }
}

fn sum_opt_u32(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(x), Some(y)) => Some(x.saturating_add(y)),
    }
}

pub fn fmt_ns(ns: u64) -> String {
    if ns < 1_000 {
        format!("{ns}ns")
    } else if ns < 1_000_000 {
        format!("{:.1}µs", ns as f64 / 1_000.0)
    } else if ns < 1_000_000_000 {
        format!("{:.2}ms", ns as f64 / 1_000_000.0)
    } else {
        format!("{:.3}s", ns as f64 / 1_000_000_000.0)
    }
}

/// Live dump for inspect / `anvil prof`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snap {
    pub samples: Vec<Sample>,
    pub counters: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_model: Option<Timing>,
}

pub struct Span {
    name: String,
    group: &'static str,
    t0: Instant,
    t0_ns: u64,
    tokens: Option<u32>,
    extra: Option<String>,
}

impl Span {
    pub fn tokens(mut self, n: u32) -> Self {
        self.tokens = Some(n);
        self
    }

    pub fn extra(mut self, extra: impl Into<String>) -> Self {
        self.extra = Some(extra.into());
        self
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let dur_ns = ns(self.t0.elapsed());
        record(Sample {
            name: self.name.clone(),
            group: self.group.into(),
            t0_ns: self.t0_ns,
            dur_ns,
            tokens: self.tokens,
            extra: self.extra.clone(),
        });
        tracing::debug!(
            target: "anvil.prof",
            name = %self.name,
            group = self.group,
            dur_ns,
            "span"
        );
    }
}

pub fn span(name: impl Into<String>, group: &'static str) -> Span {
    Span {
        name: name.into(),
        group,
        t0: Instant::now(),
        t0_ns: now_ns(),
        tokens: None,
        extra: None,
    }
}

pub fn record(sample: Sample) {
    if let Ok(mut g) = inner().lock() {
        g.push(sample);
    }
}

pub fn counter(name: &str, by: u64) {
    if let Ok(mut g) = inner().lock() {
        *g.counters.entry(name.into()).or_insert(0) += by;
    }
}

pub fn note_model(timing: Timing) {
    if let Ok(mut g) = inner().lock() {
        g.last_model = Some(timing);
    }
}

pub fn last_model() -> Option<Timing> {
    inner().lock().ok().and_then(|g| g.last_model.clone())
}

pub fn snapshot() -> Snap {
    inner()
        .lock()
        .map(|g| Snap {
            samples: g.samples.iter().cloned().collect(),
            counters: g.counters.clone(),
            last_model: g.last_model.clone(),
        })
        .unwrap_or_default()
}

pub fn estimate_tokens(text: &str) -> u32 {
    let n = text.chars().count();
    ((n + 3) / 4).max(if text.is_empty() { 0 } else { 1 }) as u32
}

/// `ANVIL_TRACE=1` or `RUST_LOG` turns on the tracing subscriber.
pub fn init() {
    let _ = epoch();
    let on = std::env::var_os("ANVIL_TRACE").is_some() || std::env::var_os("RUST_LOG").is_some();
    if !on {
        return;
    }
    let filter =
        std::env::var("RUST_LOG").unwrap_or_else(|_| "anvil=debug,anvil.prof=debug".into());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn span_lands_on_the_ring() {
        let before = snapshot().samples.len();
        {
            let _g = span("test.span", "test");
        }
        let snap = snapshot();
        assert!(snap.samples.len() > before);
        let last = snap.samples.last().unwrap();
        assert_eq!(last.name, "test.span");
        assert_eq!(last.group, "test");
    }

    #[test]
    fn compact_names_the_model_phases() {
        let t = Timing {
            wall_ns: 500_000_000,
            prefill_ns: Some(80_000_000),
            ttft_ns: Some(80_000_000),
            decode_ns: Some(400_000_000),
            reason_ns: Some(20_000_000),
            strike_ns: Some(3_000_000),
            tokens_in: Some(100),
            tokens_out: Some(40),
            tok_s: Some(100.0),
        };
        let s = t.compact();
        assert!(s.contains("ttft"), "{s}");
        assert!(s.contains("think"), "{s}");
        assert!(s.contains("tok/s"), "{s}");
        assert!(s.contains("strike"), "{s}");
    }

    #[test]
    fn tok_s_is_tokens_over_decode() {
        let mut t = Timing {
            decode_ns: Some(500_000_000),
            tokens_out: Some(25),
            ..Timing::default()
        };
        t.recompute_tok_s();
        let rate = t.tok_s.unwrap();
        assert!((rate - 50.0).abs() < 0.01, "{rate}");
    }

    #[test]
    fn fmt_ns_picks_a_unit() {
        assert_eq!(fmt_ns(400), "400ns");
        assert!(fmt_ns(2_500).contains('µ'));
        assert!(fmt_ns(12_000_000).contains("ms"));
    }

    #[test]
    fn wall_from_duration() {
        let t = Timing::wall(Duration::from_millis(3));
        assert!(t.wall_ns >= 3_000_000);
        assert!(!t.is_empty());
    }
}
