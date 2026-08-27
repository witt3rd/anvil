//! Draw saturation: unused capacity is dots. The gradient is
//! laid out across the whole track; fill is how much of it
//! shows from the left.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use crate::daemon::sat::Counters;

const TRACK_MIN: u16 = 8;
const TRACK_MAX: u16 = 40;

/// Horizontal header track. None when there is no fleet or no gap.
pub fn header_rect(left: u16, right: u16, y: u16) -> Option<Rect> {
    let gap = right.saturating_sub(left);
    if gap < TRACK_MIN {
        return None;
    }
    let width = gap.min(TRACK_MAX);
    let x = left + (gap.saturating_sub(width)) / 2;
    Some(Rect {
        x,
        y,
        width,
        height: 1,
    })
}

/// The live end of the ramp. Cool, and not the session chip
/// (`accent.primary`) or needs-you (`error`).
pub fn hue_token() -> &'static str {
    "accent.secondary"
}

pub fn fill_len(span: u16, sat: f32) -> u16 {
    if span == 0 {
        return 0;
    }
    ((sat.clamp(0.0, 1.0) * span as f32).round() as u16).min(span)
}

pub fn draw_header(
    buf: &mut Buffer,
    area: Rect,
    counters: &Counters,
    dim: Color,
    bright: Color,
    bar_bg: Color,
) {
    let Some(now) = counters.instant() else {
        return;
    };
    if area.width == 0 {
        return;
    }
    let dim_rgb = rgb_of(dim);
    let bright_rgb = rgb_of(bright);
    let hole = mix(dim_rgb, bright_rgb, 0.22);
    let filled = fill_len(area.width, now);
    for i in 0..area.width {
        let (ch, color) = if i < filled {
            cell(i, area.width, dim_rgb, bright_rgb)
        } else {
            ("·", hole)
        };
        buf.set_stringn(
            area.x + i,
            area.y,
            ch,
            1,
            Style::default().fg(color).bg(bar_bg),
        );
    }
    if let Some(mean) = counters.mean_24h() {
        let caret = fill_len(area.width.saturating_sub(1), mean);
        buf.set_stringn(
            area.x + caret,
            area.y,
            "·",
            1,
            Style::default()
                .fg(mix(dim_rgb, bright_rgb, 0.85))
                .bg(bar_bg),
        );
    }
}

/// Color at column `i` of a track `span` wide. The ramp is linear
/// across the whole span, not the filled slice.
fn cell(i: u16, span: u16, dim: [u8; 3], bright: [u8; 3]) -> (&'static str, Color) {
    let t = if span <= 1 {
        1.0
    } else {
        i as f32 / (span - 1) as f32
    };
    ("─", mix(dim, bright, t))
}

fn rgb_of(color: Color) -> [u8; 3] {
    match color {
        Color::Rgb(r, g, b) => [r, g, b],
        _ => [0x14, 0x14, 0x14],
    }
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let ch = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    Color::Rgb(ch(0), ch(1), ch(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_gap_hides_the_header_track() {
        assert!(header_rect(10, 16, 0).is_none());
        let r = header_rect(10, 50, 0).unwrap();
        assert_eq!(r.width, 40);
        assert_eq!(r.x, 10);
    }

    #[test]
    fn a_full_bar_is_the_whole_gradient() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        let counters = Counters::snapshot(1, 1);
        draw_header(
            &mut buf,
            Rect::new(0, 0, 12, 1),
            &counters,
            Color::Rgb(0x14, 0x14, 0x14),
            Color::Rgb(0x5c, 0x9c, 0xf5),
            Color::Rgb(0x14, 0x14, 0x14),
        );
        let row: String = (0..12).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(row, "────────────");
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(0x14, 0x14, 0x14));
        assert_eq!(buf[(11, 0)].fg, Color::Rgb(0x5c, 0x9c, 0xf5));
    }

    #[test]
    fn half_fill_is_the_first_half_of_the_ramp() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        let counters = Counters::snapshot(1, 2);
        draw_header(
            &mut buf,
            Rect::new(0, 0, 10, 1),
            &counters,
            Color::Rgb(0, 0, 0),
            Color::Rgb(100, 100, 100),
            Color::Rgb(0, 0, 0),
        );
        let row: String = (0..10).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(row, "─────·····");
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(0, 0, 0));
        assert_eq!(buf[(4, 0)].fg, Color::Rgb(44, 44, 44));
        assert_ne!(buf[(4, 0)].fg, Color::Rgb(100, 100, 100));
    }

    #[test]
    fn an_empty_bar_is_all_dots() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        let counters = Counters::snapshot(0, 1);
        draw_header(
            &mut buf,
            Rect::new(0, 0, 8, 1),
            &counters,
            Color::Rgb(0x14, 0x14, 0x14),
            Color::Rgb(0x5c, 0x9c, 0xf5),
            Color::Rgb(0x14, 0x14, 0x14),
        );
        let row: String = (0..8).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(row, "········");
    }

    #[test]
    fn fill_is_the_proportion() {
        assert_eq!(fill_len(20, 0.0), 0);
        assert_eq!(fill_len(20, 1.0), 20);
        assert_eq!(fill_len(20, 0.5), 10);
        assert_eq!(fill_len(0, 1.0), 0);
    }

    #[test]
    fn mix_hits_the_endpoints() {
        let a = [0, 0, 0];
        let b = [100, 200, 50];
        assert_eq!(mix(a, b, 0.0), Color::Rgb(0, 0, 0));
        assert_eq!(mix(a, b, 1.0), Color::Rgb(100, 200, 50));
        assert_eq!(mix(a, b, 0.5), Color::Rgb(50, 100, 25));
    }
}
