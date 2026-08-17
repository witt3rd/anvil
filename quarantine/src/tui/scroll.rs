//! Right-edge scrollbar. Same glyph language as herdr: track `▕`, thumb `▐`.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::Frame;

use super::theme::Face;

#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    /// Offset from the top of the content.
    pub offset: u16,
    /// Max offset from the top.
    pub max: u16,
    pub viewport: u16,
}

impl Metrics {
    pub fn visible(self) -> bool {
        self.max > 0 && self.viewport > 0
    }
}

/// One viewport onto a list of lines. Offset/stick live on the pane;
/// this is just the geometry for this paint.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub metrics: Metrics,
    pub text: Rect,
    pub start: usize,
    pub end: usize,
}

impl Window {
    pub fn open(area: Rect, nlines: usize, offset: u16, stick: bool) -> Self {
        let view_h = area.height;
        let max = nlines.saturating_sub(view_h as usize) as u16;
        let offset = if stick { max } else { offset.min(max) };
        let metrics = Metrics {
            offset,
            max,
            viewport: view_h,
        };
        let start = offset as usize;
        let end = (start + view_h as usize).min(nlines);
        Self {
            metrics,
            text: content(area, metrics),
            start,
            end,
        }
    }

    pub fn slice<'a, T>(self, items: &'a [T]) -> &'a [T] {
        let end = self.end.min(items.len());
        let start = self.start.min(end);
        &items[start..end]
    }
}

pub fn gutter(area: Rect, metrics: Metrics) -> Option<Rect> {
    if !metrics.visible() || area.width < 2 || area.height == 0 {
        return None;
    }
    Some(Rect::new(
        area.x + area.width.saturating_sub(1),
        area.y,
        1,
        area.height,
    ))
}

pub fn content(area: Rect, metrics: Metrics) -> Rect {
    match gutter(area, metrics) {
        Some(_) => Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height),
        None => area,
    }
}

pub fn render(frame: &mut Frame, area: Rect, metrics: Metrics, focused: bool) {
    let Some(track) = gutter(area, metrics) else {
        return;
    };
    let th = super::theme::t();
    let track_style = th.style(Face::ScrollTrack);
    let thumb_style = if focused {
        th.style(Face::ScrollThumb)
    } else {
        th.style(Face::ScrollTrack)
    };
    paint(frame, track, metrics, track_style, thumb_style, focused);
}

fn paint(
    frame: &mut Frame,
    track: Rect,
    metrics: Metrics,
    track_style: Style,
    thumb_style: Style,
    focused: bool,
) {
    let Some((top, len)) = thumb(metrics, track.height) else {
        return;
    };
    let buf = frame.buffer_mut();
    for y in track.y..track.y.saturating_add(track.height) {
        if y >= buf.area.y.saturating_add(buf.area.height) || track.x >= buf.area.width {
            continue;
        }
        let cell = &mut buf[(track.x, y)];
        cell.set_symbol("▕");
        cell.set_style(track_style);
    }
    let thumb_sym = if focused { "▐" } else { "▕" };
    let start = track.y.saturating_add(top);
    let end = start
        .saturating_add(len)
        .min(track.y.saturating_add(track.height));
    for y in start..end {
        if y >= buf.area.y.saturating_add(buf.area.height) {
            continue;
        }
        let cell = &mut buf[(track.x, y)];
        cell.set_symbol(thumb_sym);
        cell.set_style(thumb_style);
    }
}

fn thumb(metrics: Metrics, track_h: u16) -> Option<(u16, u16)> {
    if !metrics.visible() || track_h == 0 {
        return None;
    }
    let total = u32::from(metrics.max) + u32::from(metrics.viewport);
    if total == 0 {
        return None;
    }
    let thumb_len = ((u32::from(metrics.viewport) * u32::from(track_h)) as f32 / total as f32)
        .round()
        .clamp(1.0, f32::from(track_h)) as u16;
    let max_top = track_h.saturating_sub(thumb_len);
    let thumb_top = if max_top == 0 || metrics.max == 0 {
        0
    } else {
        ((u32::from(metrics.offset) * u32::from(max_top)) as f32 / f32::from(metrics.max))
            .round()
            .clamp(0.0, f32::from(max_top)) as u16
    };
    Some((thumb_top, thumb_len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_sticks_to_the_bottom() {
        let area = Rect::new(0, 0, 20, 5);
        let win = Window::open(area, 12, 0, true);
        assert_eq!(win.metrics.max, 7);
        assert_eq!(win.metrics.offset, 7);
        assert_eq!((win.start, win.end), (7, 12));
        assert_eq!(win.text.width, 19, "gutter reserved when scrollable");
        let stuck = Window::open(area, 4, 0, true);
        assert_eq!(stuck.metrics.max, 0);
        assert_eq!(stuck.text.width, 20, "no gutter when content fits");
    }

    #[test]
    fn thumb_sits_at_top_when_unscrolled() {
        let (top, len) = thumb(
            Metrics {
                offset: 0,
                max: 10,
                viewport: 5,
            },
            10,
        )
        .unwrap();
        assert_eq!(top, 0);
        assert!(len >= 1);
    }

    #[test]
    fn thumb_sits_at_bottom_when_fully_scrolled() {
        let (top, len) = thumb(
            Metrics {
                offset: 10,
                max: 10,
                viewport: 5,
            },
            10,
        )
        .unwrap();
        assert_eq!(top + len, 10);
    }

    #[test]
    fn hidden_when_content_fits() {
        assert!(gutter(
            Rect::new(0, 0, 20, 10),
            Metrics {
                offset: 0,
                max: 0,
                viewport: 10,
            }
        )
        .is_none());
    }
}
