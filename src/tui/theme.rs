//! Herdr chrome: Catppuccin Mocha, mauve accent as in the live seat.

use ratatui::style::{Color, Modifier, Style};

/// Colors copied from herdr's Mocha palette. Accent is mauve — that is
/// the tab pill and focused pane border in the screenshot, not Mocha blue.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub accent: Color,
    pub panel_bg: Color,
    pub active_row_bg: Color,
    pub surface0: Color,
    pub surface1: Color,
    pub surface_dim: Color,
    pub overlay0: Color,
    pub overlay1: Color,
    pub text: Color,
    pub subtext0: Color,
    pub mauve: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub blue: Color,
    pub teal: Color,
    pub peach: Color,
}

impl Palette {
    pub fn mocha() -> Self {
        Self {
            accent: Color::Rgb(203, 166, 247),
            panel_bg: Color::Rgb(24, 24, 37),
            active_row_bg: Color::Rgb(49, 50, 68),
            surface0: Color::Rgb(49, 50, 68),
            surface1: Color::Rgb(69, 71, 90),
            surface_dim: Color::Rgb(30, 30, 46),
            overlay0: Color::Rgb(108, 112, 134),
            overlay1: Color::Rgb(127, 132, 156),
            text: Color::Rgb(205, 214, 244),
            subtext0: Color::Rgb(166, 173, 200),
            mauve: Color::Rgb(203, 166, 247),
            green: Color::Rgb(166, 227, 161),
            yellow: Color::Rgb(249, 226, 175),
            red: Color::Rgb(243, 139, 168),
            blue: Color::Rgb(137, 180, 250),
            teal: Color::Rgb(148, 226, 213),
            peach: Color::Rgb(250, 179, 135),
        }
    }
}

pub fn p() -> Palette {
    Palette::mocha()
}

impl Palette {
    pub fn bg(&self) -> Style {
        Style::default().bg(self.surface_dim).fg(self.text)
    }

    pub fn header(&self) -> Style {
        Style::default().fg(self.overlay0).bg(self.surface_dim)
    }

    pub fn dim(&self) -> Style {
        Style::default().fg(self.overlay0).bg(self.surface_dim)
    }

    pub fn label(&self) -> Style {
        Style::default().fg(self.subtext0).bg(self.surface_dim)
    }

    pub fn body(&self) -> Style {
        Style::default().fg(self.text).bg(self.surface_dim)
    }

    pub fn accent_text(&self) -> Style {
        Style::default().fg(self.accent).bg(self.surface_dim)
    }

    pub fn active_row(&self) -> Style {
        Style::default().fg(self.text).bg(self.active_row_bg)
    }

    pub fn tab_active(&self) -> Style {
        Style::default()
            .fg(self.panel_bg)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn tab_idle(&self) -> Style {
        Style::default().fg(self.overlay1).bg(self.surface0)
    }

    pub fn pane_border(&self, focused: bool) -> Style {
        Style::default().fg(if focused { self.accent } else { self.surface1 })
    }

    pub fn pane_title(&self, focused: bool) -> Style {
        Style::default().fg(if focused { self.accent } else { self.overlay1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mocha_accent_is_mauve() {
        let p = Palette::mocha();
        assert_eq!(p.accent, Color::Rgb(203, 166, 247));
        assert_eq!(p.surface_dim, Color::Rgb(30, 30, 46));
    }
}
