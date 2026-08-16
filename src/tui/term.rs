//! In-pane login shell: keys to the PTY, vt100 screen drawn by ratatui.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::serve::PtyScreen;

pub fn key_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    match (key.code, key.modifiers) {
        (KeyCode::Enter, _) => Some(b"\r".to_vec()),
        (KeyCode::Tab, _) => Some(b"\t".to_vec()),
        (KeyCode::Backspace, _) => Some(vec![0x7f]),
        (KeyCode::Delete, _) => Some(b"\x1b[3~".to_vec()),
        (KeyCode::Left, _) => Some(b"\x1b[D".to_vec()),
        (KeyCode::Right, _) => Some(b"\x1b[C".to_vec()),
        (KeyCode::Up, _) => Some(b"\x1b[A".to_vec()),
        (KeyCode::Down, _) => Some(b"\x1b[B".to_vec()),
        (KeyCode::Home, _) => Some(b"\x1b[H".to_vec()),
        (KeyCode::End, _) => Some(b"\x1b[F".to_vec()),
        (KeyCode::PageUp, _) => Some(b"\x1b[5~".to_vec()),
        (KeyCode::PageDown, _) => Some(b"\x1b[6~".to_vec()),
        (KeyCode::Esc, _) => Some(b"\x1b".to_vec()),
        (KeyCode::Char(ch), KeyModifiers::CONTROL) => {
            let b = ch.to_ascii_lowercase() as u8;
            if (b'a'..=b'z').contains(&b) {
                Some(vec![b - b'a' + 1])
            } else {
                None
            }
        }
        (KeyCode::Char(ch), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            let mut buf = [0u8; 4];
            Some(ch.encode_utf8(&mut buf).as_bytes().to_vec())
        }
        _ => None,
    }
}

pub fn draw(frame: &mut Frame, area: Rect, title: &str, screen: Option<&PtyScreen>, focused: bool) {
    let title = if let Some(s) = screen {
        if s.alive {
            format!(" {title} · pty ")
        } else {
            format!(" {title} · pty · exited ")
        }
    } else {
        format!(" {title} · pty ")
    };
    let lines = match screen {
        Some(s) => s
            .lines
            .iter()
            .map(|line| Line::from(Span::raw(line.clone())))
            .collect::<Vec<_>>(),
        None => vec![Line::from(Span::styled(
            " (opening shell) ",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });
    frame.render_widget(Paragraph::new(lines).block(block), area);
    if focused {
        if let Some(s) = screen {
            if !s.alive {
                return;
            }
            let x = area.x.saturating_add(1).saturating_add(s.cursor_col);
            let y = area.y.saturating_add(1).saturating_add(s.cursor_row);
            if x < area.x.saturating_add(area.width.saturating_sub(1))
                && y < area.y.saturating_add(area.height.saturating_sub(1))
            {
                frame.set_cursor_position((x, y));
            }
        }
    }
}

pub fn hint() -> Line<'static> {
    Line::from(Span::styled(
        " pty · keys to $SHELL · Tab rail · Ctrl+Q close ",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
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
        let letter = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key_bytes(letter).as_deref(), Some(&b"a"[..]));
    }
}
