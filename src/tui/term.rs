//! In-pane login shell: keys to the PTY, vt100 screen drawn by ratatui.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::serve::PtyScreen;

pub fn key_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    let mods = key.modifiers;
    match key.code {
        KeyCode::Enter => Some(b"\r".to_vec()),
        KeyCode::Tab => Some(b"\t".to_vec()),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::PageUp => Some(b"\x1b[5~".to_vec()),
        KeyCode::PageDown => Some(b"\x1b[6~".to_vec()),
        KeyCode::Esc => Some(b"\x1b".to_vec()),
        KeyCode::Char(ch) if ch == '\u{3}' => Some(vec![0x03]),
        KeyCode::Char(ch) if mods.contains(KeyModifiers::CONTROL) => {
            let b = ch.to_ascii_lowercase() as u8;
            if (b'a'..=b'z').contains(&b) {
                Some(vec![b - b'a' + 1])
            } else {
                None
            }
        }
        KeyCode::Char(ch)
            if !mods.contains(KeyModifiers::CONTROL) && !mods.contains(KeyModifiers::ALT) =>
        {
            let mut buf = [0u8; 4];
            Some(ch.encode_utf8(&mut buf).as_bytes().to_vec())
        }
        _ => None,
    }
}

pub fn screen_lines(screen: Option<&PtyScreen>) -> Vec<Line<'static>> {
    match screen {
        Some(s) if !s.runs.is_empty() => s
            .runs
            .iter()
            .map(|runs| {
                Line::from(
                    runs.iter()
                        .map(|run| {
                            let mut style = Style::default();
                            if let Some([r, g, b]) = run.fg_rgb {
                                style = style.fg(Color::Rgb(r, g, b));
                            } else if let Some(idx) = run.fg {
                                style = style.fg(ansi_color(idx));
                            }
                            if let Some([r, g, b]) = run.bg_rgb {
                                style = style.bg(Color::Rgb(r, g, b));
                            } else if let Some(idx) = run.bg {
                                style = style.bg(ansi_color(idx));
                            }
                            if run.bold {
                                style = style.add_modifier(Modifier::BOLD);
                            }
                            if run.italic {
                                style = style.add_modifier(Modifier::ITALIC);
                            }
                            if run.underline {
                                style = style.add_modifier(Modifier::UNDERLINED);
                            }
                            if run.inverse {
                                style = style.add_modifier(Modifier::REVERSED);
                            }
                            Span::styled(run.text.clone(), style)
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect(),
        Some(s) => s
            .lines
            .iter()
            .map(|line| Line::from(Span::raw(line.clone())))
            .collect(),
        None => vec![Line::from(Span::styled(
            " (opening shell) ",
            super::theme::p().dim(),
        ))],
    }
}

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    screen: Option<&PtyScreen>,
    focused: bool,
    sel: Option<&super::select::Selection>,
    pane: &str,
) -> (Rect, Vec<Line<'static>>) {
    let title = if let Some(s) = screen {
        if s.alive {
            format!(" {title} · pty ")
        } else {
            format!(" {title} · pty · exited ")
        }
    } else {
        format!(" {title} · pty ")
    };
    let lines = screen_lines(screen);
    let inner = super::pane_body(frame, area, &title, focused);
    let cursor_row = screen.map(|s| s.cursor_row as usize).unwrap_or(0);
    let scroll = cursor_scroll(lines.len(), inner.height as usize, cursor_row);
    let end = (scroll + inner.height as usize).min(lines.len());
    let visible = if scroll == 0 && end == lines.len() {
        lines
    } else {
        lines[scroll..end].to_vec()
    };
    let shown = super::select::apply_highlight(&visible, inner, sel, pane);
    frame.render_widget(Paragraph::new(shown), inner);
    if focused {
        if let Some(s) = screen {
            if s.alive {
                let local_row = s.cursor_row.saturating_sub(scroll as u16);
                let x = inner.x.saturating_add(s.cursor_col);
                let y = inner.y.saturating_add(local_row);
                if x < inner.x.saturating_add(inner.width) && y < inner.y.saturating_add(inner.height)
                {
                    frame.set_cursor_position((x, y));
                }
            }
        }
    }
    (inner, visible)
}

/// Offset that keeps `cursor_row` inside a viewport of `view_h` rows.
pub fn cursor_scroll(nlines: usize, view_h: usize, cursor_row: usize) -> usize {
    if view_h == 0 || nlines <= view_h {
        return 0;
    }
    let max_scroll = nlines - view_h;
    cursor_row
        .saturating_sub(view_h.saturating_sub(1))
        .min(max_scroll)
}

fn ansi_color(idx: u8) -> Color {
    match idx {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        n => Color::Indexed(n),
    }
}

pub fn hint() -> Line<'static> {
    Line::from(Span::styled(
        " pty · keys to $SHELL · Tab rail · Ctrl+Q close ",
        super::theme::p().dim().add_modifier(Modifier::ITALIC),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_and_ctrl_c_are_pty_bytes() {
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_bytes(enter).as_deref(), Some(&b"\r"[..]));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_bytes(ctrl_c).as_deref(), Some(&[0x03][..]));
        let ghostty = KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(key_bytes(ghostty).as_deref(), Some(&[0x03][..]));
        let raw = KeyEvent::new(KeyCode::Char('\u{3}'), KeyModifiers::NONE);
        assert_eq!(key_bytes(raw).as_deref(), Some(&[0x03][..]));
        let letter = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key_bytes(letter).as_deref(), Some(&b"a"[..]));
    }

    #[test]
    fn cursor_scroll_keeps_the_prompt_in_view() {
        assert_eq!(cursor_scroll(24, 24, 23), 0);
        assert_eq!(cursor_scroll(24, 6, 23), 18);
        assert_eq!(cursor_scroll(24, 6, 0), 0);
        assert_eq!(cursor_scroll(24, 6, 5), 0);
        assert_eq!(cursor_scroll(8, 0, 7), 0);
    }
}
