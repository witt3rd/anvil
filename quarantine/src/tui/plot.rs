//! Stats pane: phase-based agent profiling. The event log is the
//! source; this is a projection, not a ring of fiber ticks.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};
use ratatui::Frame;

use super::activity::Activity;
use super::theme::{self, Face};
use super::App;
use crate::frame::Event;
use crate::prof::{self, Timing};
use crate::stats::{self, Fold, Phase, Trace, TraceRow};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, id: &str, focused: bool) {
    let of = app
        .rail
        .as_ref()
        .and_then(|r| r.plot_of(id))
        .unwrap_or(id)
        .to_string();
    let events = events_for(app, &of);
    let fold = stats::fold(&events, "");
    let mut trace = stats::trace(&events);
    let live = app
        .activity_of
        .as_deref()
        .is_some_and(|s| s == of)
        && app.activity.is_some();
    if live {
        if let Some(act) = app.activity.as_ref() {
            overlay_live(&mut trace, act, app.prof.last_model.as_ref());
        }
    }
    let many = app
        .rail
        .as_ref()
        .is_some_and(|r| r.stage_members().len() > 1);
    let title = if many {
        format!("{id} · stats {of}")
    } else {
        String::new()
    };
    let inner = super::pane_body(frame, area, &title, focused);
    if inner.width < 8 || inner.height < 3 {
        return;
    }

    let mid_h = if inner.height >= 14 {
        6
    } else if inner.height >= 10 {
        5
    } else if inner.height >= 7 {
        4
    } else {
        0
    };
    let life_h = if inner.height >= 5 { 2 } else { 1 };
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(life_h),
            Constraint::Length(mid_h),
            Constraint::Min(1),
        ])
        .split(inner);

    draw_lifecycle(frame, parts[0], &trace.last_turn);
    if mid_h >= 4 {
        draw_metrics(frame, parts[1], &trace, &fold);
        draw_table(frame, parts[2], app, id, &trace.rows, focused);
    } else {
        draw_compact(frame, parts[2], &trace, &fold);
    }
}

fn events_for(app: &App, of: &str) -> Vec<Event> {
    if of == app.session_id() && !app.log_events.is_empty() {
        return app.log_events.clone();
    }
    if let Some(root) = &app.frame {
        if let Ok(ev) = root.load_events(of) {
            return ev;
        }
    }
    app.log_events.clone()
}

fn overlay_live(trace: &mut Trace, act: &Activity, last_model: Option<&Timing>) {
    if act.phase == "idle" {
        return;
    }
    let turn = trace.last_turn.last().map(|r| r.turn).unwrap_or(1).max(1);
    let mut next = trace.rows.last().map(|r| r.id + 1).unwrap_or(1);
    if last_model
        .and_then(|t| t.ttft_ns.or(t.prefill_ns))
        .is_some()
        && !trace.last_turn.iter().any(|r| r.phase == Phase::Prefill)
    {
        if let Some(t) = last_model {
            if let Some(n) = t.ttft_ns.or(t.prefill_ns) {
                let row = TraceRow {
                    id: next,
                    turn,
                    phase: Phase::Prefill,
                    dur_ns: n,
                    tokens: t.tokens_in.or(t.tokens_cache_read),
                    tok_s: None,
                    cache_hit: None,
                    status: "live".into(),
                };
                next += 1;
                trace.last_turn.push(row.clone());
                trace.rows.push(row);
            }
        }
    }
    match act.phase.as_str() {
        "think" | "thinking" | "prefill" | "waiting" => {
            if !trace.last_turn.iter().any(|r| r.phase == Phase::Think) {
                let dur = act
                    .steps
                    .iter()
                    .rev()
                    .find(|s| matches!(s.kind, super::activity::StepKind::Think | super::activity::StepKind::Prefill))
                    .map(|s| s.dur.unwrap_or_else(|| s.t0.elapsed()))
                    .unwrap_or_else(|| act.elapsed());
                let row = TraceRow {
                    id: next,
                    turn,
                    phase: Phase::Think,
                    dur_ns: prof::ns(dur),
                    tokens: Some(act.tokens).filter(|n| *n > 0),
                    tok_s: None,
                    cache_hit: None,
                    status: "STREAMING".into(),
                };
                trace.last_turn.push(row.clone());
                trace.rows.push(row);
            }
        }
        "decode" => {
            if !trace.last_turn.iter().any(|r| r.phase == Phase::Decode) {
                let row = TraceRow {
                    id: next,
                    turn,
                    phase: Phase::Decode,
                    dur_ns: prof::ns(act.elapsed()),
                    tokens: Some(act.tokens).filter(|n| *n > 0),
                    tok_s: None,
                    cache_hit: None,
                    status: "STREAMING".into(),
                };
                trace.last_turn.push(row.clone());
                trace.rows.push(row);
            }
        }
        "tool" | "striking" => {
            if !trace.last_turn.iter().any(|r| r.phase == Phase::Tool) {
                let last = act
                    .steps
                    .iter()
                    .rev()
                    .find(|s| s.kind == super::activity::StepKind::Tool);
                let dur = last
                    .and_then(|s| s.dur)
                    .unwrap_or_else(|| act.elapsed());
                let status = last
                    .map(|s| format!("tool: {}", s.title))
                    .unwrap_or_else(|| "tool".into());
                let row = TraceRow {
                    id: next,
                    turn,
                    phase: Phase::Tool,
                    dur_ns: prof::ns(dur),
                    tokens: None,
                    tok_s: None,
                    cache_hit: None,
                    status,
                };
                trace.last_turn.push(row.clone());
                trace.rows.push(row);
            }
        }
        _ => {}
    }
}

fn phase_face(phase: Phase) -> Face {
    match phase {
        Phase::Prefill => Face::PlotPrefill,
        Phase::Think => Face::PlotThink,
        Phase::Decode => Face::PlotDecode,
        Phase::Tool => Face::PlotTool,
    }
}

fn draw_lifecycle(frame: &mut Frame, area: Rect, rows: &[TraceRow]) {
    let th = theme::p();
    let inner = headed(frame, area, "lifecycle");
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(" no turn yet ", th.style(Face::PlotMute))),
            inner,
        );
        return;
    }
    let line = lifecycle_line(rows, inner.width);
    frame.render_widget(Paragraph::new(line), inner);
}

fn lifecycle_line(rows: &[TraceRow], width: u16) -> Line<'static> {
    let th = theme::p();
    let total = rows.iter().map(|r| r.dur_ns.max(1)).sum::<u64>().max(1);
    let labels: Vec<String> = rows
        .iter()
        .map(|r| format!("{} {}", r.phase.label(), prof::fmt_ns(r.dur_ns)))
        .collect();
    let label_w: usize = labels.iter().map(|s| s.chars().count() + 3).sum();
    let bar_w = (width as usize).saturating_sub(label_w).max(rows.len());
    let shares = shares(
        &rows.iter().map(|r| r.dur_ns.max(1)).collect::<Vec<_>>(),
        bar_w as u16,
        total,
    );
    let mut spans = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let face = phase_face(row.phase);
        if i > 0 {
            spans.push(Span::styled(" ", th.style(Face::PlotMute)));
        }
        spans.push(Span::styled(
            format!("{} ", labels[i]),
            th.style(face),
        ));
        let n = shares.get(i).copied().unwrap_or(1).max(1) as usize;
        spans.push(Span::styled(
            "■".repeat(n),
            th.style(face).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

/// Largest-remainder so the painted blocks sum to `width`.
pub fn shares(durs: &[u64], width: u16, total: u64) -> Vec<u16> {
    if durs.is_empty() || width == 0 || total == 0 {
        return vec![0; durs.len()];
    }
    let w = u64::from(width);
    let mut raw: Vec<(usize, u16, f64)> = durs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let exact = (*d as f64) * (w as f64) / (total as f64);
            let base = exact.floor() as u16;
            (i, base.max(1).min(width), exact - f64::from(base))
        })
        .collect();
    let mut used: u16 = raw.iter().map(|(_, b, _)| *b).sum();
    if used > width {
        let overflow = used - width;
        let mut left = overflow;
        for (_, b, _) in raw.iter_mut().rev() {
            if left == 0 {
                break;
            }
            if *b > 1 {
                let take = (*b - 1).min(left);
                *b -= take;
                left -= take;
            }
        }
        used = raw.iter().map(|(_, b, _)| *b).sum();
    }
    if used < width {
        raw.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        let mut extra = width - used;
        let n = raw.len();
        let mut i = 0;
        while extra > 0 && n > 0 {
            raw[i].1 = raw[i].1.saturating_add(1);
            extra -= 1;
            i = (i + 1) % n;
        }
        raw.sort_by_key(|(i, _, _)| *i);
    }
    let mut out = vec![0u16; durs.len()];
    for (i, b, _) in raw {
        out[i] = b;
    }
    out
}

fn draw_metrics(frame: &mut Frame, area: Rect, trace: &Trace, fold: &Fold) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);
    draw_callouts(frame, cols[0], trace);
    draw_health(frame, cols[1], trace, fold);
    draw_velocity(frame, cols[2], trace, fold);
}

fn headed(frame: &mut Frame, area: Rect, title: &str) -> Rect {
    let th = theme::p();
    if area.height == 0 {
        return area;
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {title}"),
            th.style(Face::PlotMute),
        )),
        Rect::new(area.x, area.y, area.width, 1),
    );
    Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    )
}

fn draw_callouts(frame: &mut Frame, area: Rect, trace: &Trace) {
    let th = theme::p();
    let inner = headed(frame, area, "live");
    let p95 = stats::percentile(&trace.ttfts, 95.0);
    let ttft = match (trace.last_ttft(), p95) {
        (Some(n), Some(p)) if trace.ttfts.len() > 1 => {
            format!("TTFT    {}  (p95 {})", prof::fmt_ns(n), prof::fmt_ns(p))
        }
        (Some(n), _) => format!("TTFT    {}", prof::fmt_ns(n)),
        _ => "TTFT    —".into(),
    };
    let think = trace
        .last_think()
        .map(|n| format!("Think   {}", prof::fmt_ns(n)))
        .unwrap_or_else(|| "Think   —".into());
    let decode = trace
        .last_decode()
        .map(|n| format!("Decode  {}", prof::fmt_ns(n)))
        .unwrap_or_else(|| "Decode  —".into());
    let lines = vec![
        Line::from(Span::styled(ttft, th.style(Face::PlotStat))),
        Line::from(Span::styled(think, th.style(Face::PlotThink))),
        Line::from(Span::styled(decode, th.style(Face::PlotDecode))),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_health(frame: &mut Frame, area: Rect, trace: &Trace, fold: &Fold) {
    let th = theme::p();
    let inner = headed(frame, area, "cache · context");
    let hit = fold.usage.cache_hit().or_else(|| {
        trace
            .last_of(Phase::Prefill)
            .and_then(|r| r.cache_hit)
    });
    let hit_line = match hit {
        Some(h) => {
            let bar = fill_bar(h, 8);
            format!("KV hit  {:>4.0}% {bar}", h * 100.0)
        }
        None => "KV hit  —".into(),
    };
    let ctx = fold.context.projected.or(fold.context.pressure);
    let ctx_line = match (ctx, fold.context.window) {
        (Some(p), Some(w)) if w > 0 => {
            let frac = f64::from(p) / f64::from(w);
            format!(
                "Ctx     {}/{} {}",
                stats::fmt_tokens(p),
                stats::fmt_tokens(w),
                fill_bar(frac, 8)
            )
        }
        (Some(p), _) => format!("Ctx     ~{}", stats::fmt_tokens(p)),
        _ => "Ctx     —".into(),
    };
    let strike = trace
        .last_strike()
        .map(|n| format!("Strike  {}", prof::fmt_ns(n)))
        .unwrap_or_else(|| "Strike  —".into());
    let hit_face = match hit {
        Some(h) if h < 0.4 => Face::PlotGaugeHot,
        Some(h) if h < 0.7 => Face::PlotGaugeWarn,
        Some(_) => Face::PlotGauge,
        None => Face::PlotMute,
    };
    let ctx_face = match (ctx, fold.context.window) {
        (Some(p), Some(w)) if w > 0 && f64::from(p) / f64::from(w) > 0.85 => Face::PlotGaugeHot,
        (Some(p), Some(w)) if w > 0 && f64::from(p) / f64::from(w) > 0.7 => Face::PlotGaugeWarn,
        (Some(_), _) => Face::PlotGauge,
        _ => Face::PlotMute,
    };
    let lines = vec![
        Line::from(Span::styled(hit_line, th.style(hit_face))),
        Line::from(Span::styled(ctx_line, th.style(ctx_face))),
        Line::from(Span::styled(strike, th.style(Face::PlotTool))),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn fill_bar(frac: f64, width: usize) -> String {
    let n = width.max(1);
    let filled = ((frac.clamp(0.0, 1.0) * n as f64).round() as usize).min(n);
    format!("{}{}", "█".repeat(filled), "░".repeat(n - filled))
}

fn draw_velocity(frame: &mut Frame, area: Rect, trace: &Trace, fold: &Fold) {
    let th = theme::p();
    let inner = headed(frame, area, "tok/s");
    if inner.height == 0 {
        return;
    }
    let rate = trace.last_tok_s().or_else(|| fold.stats.tok_s());
    let head = match rate {
        Some(s) => format!("{s:.1} tok/s"),
        None => "— tok/s".into(),
    };
    let head_h = 1u16;
    let chart_h = inner.height.saturating_sub(head_h);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(head_h), Constraint::Min(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Span::styled(head, th.style(Face::PlotStat))),
        chunks[0],
    );
    if chart_h < 2 || trace.tok_s.len() < 2 {
        if chart_h > 0 && trace.tok_s.len() < 2 {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    " need 2+ decode steps ",
                    th.style(Face::PlotMute),
                )),
                chunks[1],
            );
        }
        return;
    }
    let data: Vec<(f64, f64)> = trace
        .tok_s
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v))
        .collect();
    let max_y = data
        .iter()
        .map(|(_, y)| *y)
        .fold(1.0_f64, f64::max)
        .max(1.0)
        * 1.1;
    let xmax = (data.len().saturating_sub(1) as f64).max(1.0);
    let ds = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(th.style(Face::PlotSpark))
        .data(&data);
    let chart = Chart::new(vec![ds])
        .x_axis(Axis::default().bounds([0.0, xmax]))
        .y_axis(
            Axis::default()
                .bounds([0.0, max_y])
                .title(Span::styled(
                    format!("0-{:.0}", max_y),
                    th.style(Face::PlotMute),
                )),
        );
    frame.render_widget(chart, chunks[1]);
}

fn draw_table(
    frame: &mut Frame,
    area: Rect,
    app: &mut App,
    id: &str,
    rows: &[TraceRow],
    focused: bool,
) {
    let inner = headed(frame, area, "trace");
    if inner.height == 0 {
        return;
    }
    let header_h = u16::from(inner.height >= 2);
    let body_area = Rect::new(
        inner.x,
        inner.y.saturating_add(header_h),
        inner.width,
        inner.height.saturating_sub(header_h),
    );
    let win = app.scroll_window(id, body_area, rows.len());
    if header_h > 0 {
        frame.render_widget(
            table_header(win.text.width),
            Rect::new(inner.x, inner.y, win.text.width, 1),
        );
    }
    let lines: Vec<Line> = rows
        .iter()
        .map(|r| table_row(r, win.text.width))
        .collect();
    super::paint_window(
        frame,
        body_area,
        &lines,
        win,
        app,
        id,
        focused,
        ratatui::style::Style::default(),
        false,
    );
}

fn table_header(width: u16) -> Line<'static> {
    let th = theme::p();
    Line::from(Span::styled(
        fit_cols(width, "ID", "Phase", "Duration", "Tokens", "tok/s", "Status"),
        th.style(Face::PlotMute).add_modifier(Modifier::BOLD),
    ))
}

fn table_row(row: &TraceRow, width: u16) -> Line<'static> {
    let th = theme::p();
    let toks = row
        .tokens
        .map(stats::fmt_tokens)
        .unwrap_or_else(|| "—".into());
    let rate = row
        .tok_s
        .map(|s| format!("{s:.1}"))
        .unwrap_or_else(|| "—".into());
    Line::from(Span::styled(
        fit_cols(
            width,
            &format!("#{}", row.id),
            row.phase.label(),
            &prof::fmt_ns(row.dur_ns),
            &toks,
            &rate,
            &row.status,
        ),
        th.style(phase_face(row.phase)),
    ))
}

fn fit_cols(
    width: u16,
    id: &str,
    phase: &str,
    dur: &str,
    toks: &str,
    rate: &str,
    status: &str,
) -> String {
    let w = width as usize;
    if w < 28 {
        return format!("{id} {phase} {dur}");
    }
    if w < 48 {
        return format!("{id:<4} {phase:<8} {dur:<10} {status}");
    }
    format!("{id:<4} {phase:<8} {dur:<10} {toks:<8} {rate:<8} {status}")
}

fn draw_compact(frame: &mut Frame, area: Rect, trace: &Trace, fold: &Fold) {
    let th = theme::p();
    let ttft = trace
        .last_ttft()
        .map(prof::fmt_ns)
        .unwrap_or_else(|| "—".into());
    let rate = trace
        .last_tok_s()
        .or_else(|| fold.stats.tok_s())
        .map(|s| format!("{s:.1} tok/s"))
        .unwrap_or_else(|| "—".into());
    let hit = fold
        .usage
        .cache_hit()
        .map(|h| format!("cache {:.0}%", h * 100.0))
        .unwrap_or_else(|| "cache —".into());
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" ttft {ttft}  {rate}  {hit}"),
            th.style(Face::PlotStat),
        )),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_sum_to_width_and_keep_every_phase() {
        let durs = vec![450_000_000u64, 9_900_000_000, 254_000_000, 1_120_000_000];
        let total: u64 = durs.iter().sum();
        let got = shares(&durs, 40, total);
        assert_eq!(got.iter().sum::<u16>(), 40);
        assert!(got.iter().all(|n| *n >= 1));
        assert!(got[1] > got[0], "think should dominate {got:?}");
    }

    #[test]
    fn fill_bar_clamps() {
        assert_eq!(fill_bar(0.0, 4), "░░░░");
        assert_eq!(fill_bar(1.0, 4), "████");
        assert_eq!(fill_bar(0.5, 4), "██░░");
    }
}
