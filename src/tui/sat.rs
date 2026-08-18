//! Draw saturation: a track of unused capacity, a fill of now,
//! a stain of the last day, and a glyph that is the energy band.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::daemon::sat::{self, Counters};

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

pub fn h_glyph(agents: u32) -> &'static str {
    match sat::band(agents) {
        0 => " ",
        1 => "─",
        2 => "─",
        3 => "━",
        4 => "━",
        5 => "═",
        _ => "▬",
    }
}

pub fn v_glyph(agents: u32) -> &'static str {
    match sat::band(agents) {
        0 => " ",
        1 => "│",
        2 => "┃",
        3 => "║",
        4 => "┃",
        5 => "║",
        _ => "▌",
    }
}

/// Fill stays quiet. Higher bands warm slightly; never a slab.
pub fn fill_token(agents: u32) -> &'static str {
    if sat::band(agents) >= 6 {
        "warning"
    } else {
        "text.muted"
    }
}

/// Empty track. Must read on `bg.panel` — `border.subtle` does not.
pub fn track_token() -> &'static str {
    "text.muted"
}

pub fn stain_token() -> &'static str {
    "text.dim"
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
    track: Style,
    fill: Style,
    stain: Style,
) {
    let Some(now) = counters.instant() else {
        return;
    };
    if area.width < 2 {
        return;
    }
    let inner = area.width.saturating_sub(2);
    let glyph = h_glyph(counters.agents);
    let filled = fill_len(inner, now);
    buf.set_stringn(area.x, area.y, "[", 1, track);
    buf.set_stringn(area.x + area.width - 1, area.y, "]", 1, track);
    for i in 0..inner {
        let x = area.x + 1 + i;
        if i < filled {
            buf.set_stringn(x, area.y, glyph, 1, fill);
        } else {
            buf.set_stringn(x, area.y, "·", 1, track);
        }
    }
    if let Some(mean) = counters.mean_24h() {
        let caret = fill_len(inner.saturating_sub(1), mean);
        buf.set_stringn(area.x + 1 + caret, area.y, "│", 1, stain);
    }
}

pub fn draw_strip(
    buf: &mut Buffer,
    area: Rect,
    counters: &Counters,
    track: Style,
    fill: Style,
    stain: Style,
) {
    let Some(now) = counters.instant() else {
        return;
    };
    if area.height == 0 || area.width == 0 {
        return;
    }
    let glyph = v_glyph(counters.agents);
    let filled = fill_len(area.height, now);
    let bottom = area.bottom();
    for row in 0..area.height {
        let y = area.y + row;
        let from_bottom = bottom.saturating_sub(y + 1);
        let (ch, style) = if from_bottom < filled {
            (glyph, fill)
        } else {
            ("·", track)
        };
        buf.set_stringn(area.x, y, ch, 1, style);
    }
    if let Some(mean) = counters.mean_24h() {
        let caret = fill_len(area.height.saturating_sub(1), mean);
        let y = bottom.saturating_sub(caret + 1);
        if y >= area.y {
            buf.set_stringn(area.x, y, "─", 1, stain);
        }
    }
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
    fn one_and_a_hundred_use_different_glyphs() {
        assert_eq!(h_glyph(1), "─");
        assert_eq!(h_glyph(100), "▬");
        assert_ne!(h_glyph(1), h_glyph(100));
        assert_ne!(v_glyph(1), v_glyph(100));
    }

    #[test]
    fn a_full_bar_paints_brackets_and_fill() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        let counters = Counters::snapshot(1, 1);
        draw_header(
            &mut buf,
            Rect::new(0, 0, 12, 1),
            &counters,
            Style::default(),
            Style::default(),
            Style::default(),
        );
        let row: String = (0..12).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(row.starts_with('['), "{row}");
        assert!(row.ends_with(']'), "{row}");
        assert!(row.contains('─'), "{row}");
    }

    #[test]
    fn fill_is_the_proportion() {
        assert_eq!(fill_len(20, 0.0), 0);
        assert_eq!(fill_len(20, 1.0), 20);
        assert_eq!(fill_len(20, 0.5), 10);
        assert_eq!(fill_len(0, 1.0), 0);
    }
}
