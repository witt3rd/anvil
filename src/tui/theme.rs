//! Named faces for every smith surface. A pack fills the inks; a face
//! is a role (`message.user.field`, `hint.key`). Draw only asks for faces.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

/// Color primitives. Faces bind to these, not to hex, so a pack retints
/// the whole seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ink {
    Canvas,
    Raised,
    Inset,
    Overlay,
    Faint,
    Ink,
    Mute,
    Accent,
    OnAccent,
    Good,
    Warn,
    Bad,
    Info,
    Peach,
}

/// Every painted role. Adding a widget means adding a face here first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Face {
    Canvas,
    Rail,
    RailHeader,
    RailRow,
    RailRowActive,
    RailDotSession,
    RailDotPty,
    RailDotEdit,
    RailDotLog,
    RailDotClock,
    RailDotIdle,
    TabBar,
    TabActive,
    TabIdle,
    TabAdd,
    PaneBorder,
    PaneBorderFocus,
    PaneTitle,
    PaneTitleFocus,
    PaneField,
    MessageUserField,
    MessageUserTag,
    MessageUserInk,
    MessageAgentField,
    MessageAgentTag,
    MessageAgentInk,
    MessageThinkInk,
    MessageStrikeOk,
    MessageStrikeFail,
    MessageSee,
    MessageMute,
    ComposePrompt,
    ComposeInput,
    HintBar,
    HintKey,
    HintSep,
    HintLabel,
    HintPill,
    StatusInk,
    PickerField,
    PickerHit,
    PickerHitActive,
    EditEmpty,
    EditCursor,
    Trajectory,
}

impl Face {
    pub const ALL: &'static [Face] = &[
        Face::Canvas,
        Face::Rail,
        Face::RailHeader,
        Face::RailRow,
        Face::RailRowActive,
        Face::RailDotSession,
        Face::RailDotPty,
        Face::RailDotEdit,
        Face::RailDotLog,
        Face::RailDotClock,
        Face::RailDotIdle,
        Face::TabBar,
        Face::TabActive,
        Face::TabIdle,
        Face::TabAdd,
        Face::PaneBorder,
        Face::PaneBorderFocus,
        Face::PaneTitle,
        Face::PaneTitleFocus,
        Face::PaneField,
        Face::MessageUserField,
        Face::MessageUserTag,
        Face::MessageUserInk,
        Face::MessageAgentField,
        Face::MessageAgentTag,
        Face::MessageAgentInk,
        Face::MessageThinkInk,
        Face::MessageStrikeOk,
        Face::MessageStrikeFail,
        Face::MessageSee,
        Face::MessageMute,
        Face::ComposePrompt,
        Face::ComposeInput,
        Face::HintBar,
        Face::HintKey,
        Face::HintSep,
        Face::HintLabel,
        Face::HintPill,
        Face::StatusInk,
        Face::PickerField,
        Face::PickerHit,
        Face::PickerHitActive,
        Face::EditEmpty,
        Face::EditCursor,
        Face::Trajectory,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Face::Canvas => "canvas",
            Face::Rail => "rail",
            Face::RailHeader => "rail.header",
            Face::RailRow => "rail.row",
            Face::RailRowActive => "rail.row.active",
            Face::RailDotSession => "rail.dot.session",
            Face::RailDotPty => "rail.dot.pty",
            Face::RailDotEdit => "rail.dot.edit",
            Face::RailDotLog => "rail.dot.log",
            Face::RailDotClock => "rail.dot.clock",
            Face::RailDotIdle => "rail.dot.idle",
            Face::TabBar => "tab.bar",
            Face::TabActive => "tab.active",
            Face::TabIdle => "tab.idle",
            Face::TabAdd => "tab.add",
            Face::PaneBorder => "pane.border",
            Face::PaneBorderFocus => "pane.border.focus",
            Face::PaneTitle => "pane.title",
            Face::PaneTitleFocus => "pane.title.focus",
            Face::PaneField => "pane.field",
            Face::MessageUserField => "message.user.field",
            Face::MessageUserTag => "message.user.tag",
            Face::MessageUserInk => "message.user.ink",
            Face::MessageAgentField => "message.agent.field",
            Face::MessageAgentTag => "message.agent.tag",
            Face::MessageAgentInk => "message.agent.ink",
            Face::MessageThinkInk => "message.think.ink",
            Face::MessageStrikeOk => "message.strike.ok",
            Face::MessageStrikeFail => "message.strike.fail",
            Face::MessageSee => "message.see",
            Face::MessageMute => "message.mute",
            Face::ComposePrompt => "compose.prompt",
            Face::ComposeInput => "compose.input",
            Face::HintBar => "hint.bar",
            Face::HintKey => "hint.key",
            Face::HintSep => "hint.sep",
            Face::HintLabel => "hint.label",
            Face::HintPill => "hint.pill",
            Face::StatusInk => "status.ink",
            Face::PickerField => "picker.field",
            Face::PickerHit => "picker.hit",
            Face::PickerHitActive => "picker.hit.active",
            Face::EditEmpty => "edit.empty",
            Face::EditCursor => "edit.cursor",
            Face::Trajectory => "trajectory",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Face::ALL.iter().copied().find(|f| f.as_str() == name)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FaceSpec {
    pub fg: Ink,
    pub bg: Ink,
    pub bold: bool,
}

#[derive(Debug, Clone)]
pub struct Theme {
    #[allow(dead_code)]
    pub pack: String,
    pub inks: BTreeMap<Ink, Color>,
    pub faces: BTreeMap<Face, FaceSpec>,
}

impl Theme {
    pub fn mocha() -> Self {
        Self::from_inks(
            "mocha",
            [
                (Ink::Canvas, rgb(30, 30, 46)),
                (Ink::Raised, rgb(49, 50, 68)),
                (Ink::Inset, rgb(24, 24, 37)),
                (Ink::Overlay, rgb(69, 71, 90)),
                (Ink::Faint, rgb(108, 112, 134)),
                (Ink::Ink, rgb(205, 214, 244)),
                (Ink::Mute, rgb(166, 173, 200)),
                (Ink::Accent, rgb(203, 166, 247)),
                (Ink::OnAccent, rgb(24, 24, 37)),
                (Ink::Good, rgb(166, 227, 161)),
                (Ink::Warn, rgb(249, 226, 175)),
                (Ink::Bad, rgb(243, 139, 168)),
                (Ink::Info, rgb(137, 180, 250)),
                (Ink::Peach, rgb(250, 179, 135)),
            ],
        )
    }

    pub fn terminal() -> Self {
        Self::from_inks(
            "terminal",
            [
                (Ink::Canvas, Color::Reset),
                (Ink::Raised, Color::DarkGray),
                (Ink::Inset, Color::Black),
                (Ink::Overlay, Color::DarkGray),
                (Ink::Faint, Color::Gray),
                (Ink::Ink, Color::White),
                (Ink::Mute, Color::Gray),
                (Ink::Accent, Color::Magenta),
                (Ink::OnAccent, Color::Black),
                (Ink::Good, Color::Green),
                (Ink::Warn, Color::Yellow),
                (Ink::Bad, Color::LightRed),
                (Ink::Info, Color::Cyan),
                (Ink::Peach, Color::Yellow),
            ],
        )
    }

    pub fn named(pack: &str) -> Self {
        match pack {
            "terminal" => Self::terminal(),
            _ => Self::mocha(),
        }
    }

    fn from_inks(pack: &str, inks: impl IntoIterator<Item = (Ink, Color)>) -> Self {
        let inks: BTreeMap<Ink, Color> = inks.into_iter().collect();
        let faces = default_faces();
        Self {
            pack: pack.into(),
            inks,
            faces,
        }
    }

    pub fn from_config(cfg: &crate::config::ThemeConfig) -> Self {
        let mut theme = Self::named(cfg.pack.as_deref().unwrap_or("mocha"));
        for (name, value) in &cfg.ink {
            if let Some(ink) = parse_ink(name) {
                theme.inks.insert(ink, parse_color(value));
            }
        }
        for (name, face) in &cfg.face {
            let Some(slot) = Face::parse(name) else {
                continue;
            };
            let mut spec = theme.faces.get(&slot).copied().unwrap_or(FaceSpec {
                fg: Ink::Ink,
                bg: Ink::Canvas,
                bold: false,
            });
            if let Some(fg) = face.fg.as_deref().and_then(parse_ink) {
                spec.fg = fg;
            }
            if let Some(bg) = face.bg.as_deref().and_then(parse_ink) {
                spec.bg = bg;
            }
            if let Some(bold) = face.bold {
                spec.bold = bold;
            }
            theme.faces.insert(slot, spec);
        }
        theme
    }

    pub fn color(&self, ink: Ink) -> Color {
        self.inks.get(&ink).copied().unwrap_or(Color::Reset)
    }

    pub fn spec(&self, face: Face) -> FaceSpec {
        self.faces.get(&face).copied().unwrap_or(FaceSpec {
            fg: Ink::Ink,
            bg: Ink::Canvas,
            bold: false,
        })
    }

    #[allow(dead_code)]
    pub fn fg(&self, face: Face) -> Color {
        self.color(self.spec(face).fg)
    }

    pub fn bg_of(&self, face: Face) -> Color {
        self.color(self.spec(face).bg)
    }

    pub fn style(&self, face: Face) -> Style {
        let spec = self.spec(face);
        let mut style = Style::default()
            .fg(self.color(spec.fg))
            .bg(self.color(spec.bg));
        if spec.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        style
    }

    pub fn bg(&self) -> Style {
        self.style(Face::Canvas)
    }
    pub fn dim(&self) -> Style {
        self.style(Face::StatusInk)
    }
    #[allow(dead_code)]
    pub fn header(&self) -> Style {
        self.style(Face::RailHeader)
    }
    #[allow(dead_code)]
    pub fn label(&self) -> Style {
        self.style(Face::RailRow)
    }
    pub fn body(&self) -> Style {
        self.style(Face::PaneField)
    }
    pub fn accent_text(&self) -> Style {
        self.style(Face::ComposePrompt)
    }
    #[allow(dead_code)]
    pub fn active_row(&self) -> Style {
        self.style(Face::RailRowActive)
    }
    pub fn tab_active(&self) -> Style {
        self.style(Face::TabActive)
    }
    pub fn tab_idle(&self) -> Style {
        self.style(Face::TabIdle)
    }

    pub fn pane_border(&self, focused: bool) -> Style {
        self.style(if focused {
            Face::PaneBorderFocus
        } else {
            Face::PaneBorder
        })
    }

    pub fn pane_title(&self, focused: bool) -> Style {
        self.style(if focused {
            Face::PaneTitleFocus
        } else {
            Face::PaneTitle
        })
    }
}

fn default_faces() -> BTreeMap<Face, FaceSpec> {
    use Face::*;
    let f = |fg, bg| FaceSpec {
        fg,
        bg,
        bold: false,
    };
    let b = |fg, bg| FaceSpec { fg, bg, bold: true };
    [
        (Canvas, f(Ink::Ink, Ink::Canvas)),
        (Rail, f(Ink::Ink, Ink::Canvas)),
        (RailHeader, f(Ink::Faint, Ink::Canvas)),
        (RailRow, f(Ink::Mute, Ink::Canvas)),
        (RailRowActive, f(Ink::Ink, Ink::Raised)),
        (RailDotSession, f(Ink::Info, Ink::Canvas)),
        (RailDotPty, f(Ink::Good, Ink::Canvas)),
        (RailDotEdit, f(Ink::Peach, Ink::Canvas)),
        (RailDotLog, f(Ink::Info, Ink::Canvas)),
        (RailDotClock, f(Ink::Accent, Ink::Canvas)),
        (RailDotIdle, f(Ink::Faint, Ink::Canvas)),
        (TabBar, f(Ink::Mute, Ink::Inset)),
        (TabActive, b(Ink::OnAccent, Ink::Accent)),
        (TabIdle, f(Ink::Mute, Ink::Raised)),
        (TabAdd, f(Ink::Faint, Ink::Inset)),
        (PaneBorder, f(Ink::Overlay, Ink::Canvas)),
        (PaneBorderFocus, f(Ink::Accent, Ink::Canvas)),
        (PaneTitle, f(Ink::Faint, Ink::Canvas)),
        (PaneTitleFocus, f(Ink::Accent, Ink::Canvas)),
        (PaneField, f(Ink::Ink, Ink::Canvas)),
        (MessageUserField, f(Ink::Ink, Ink::Raised)),
        (MessageUserTag, b(Ink::Mute, Ink::Raised)),
        (MessageUserInk, f(Ink::Ink, Ink::Raised)),
        (MessageAgentField, f(Ink::Ink, Ink::Canvas)),
        (MessageAgentTag, b(Ink::Mute, Ink::Canvas)),
        (MessageAgentInk, f(Ink::Ink, Ink::Canvas)),
        (MessageThinkInk, f(Ink::Warn, Ink::Canvas)),
        (MessageStrikeOk, f(Ink::Good, Ink::Canvas)),
        (MessageStrikeFail, f(Ink::Bad, Ink::Canvas)),
        (MessageSee, f(Ink::Accent, Ink::Canvas)),
        (MessageMute, f(Ink::Faint, Ink::Canvas)),
        (ComposePrompt, f(Ink::Accent, Ink::Canvas)),
        (ComposeInput, f(Ink::Ink, Ink::Canvas)),
        (HintBar, f(Ink::Faint, Ink::Inset)),
        (HintKey, f(Ink::Mute, Ink::Inset)),
        (HintSep, f(Ink::Faint, Ink::Inset)),
        (HintLabel, f(Ink::Faint, Ink::Inset)),
        (HintPill, f(Ink::Mute, Ink::Raised)),
        (StatusInk, f(Ink::Faint, Ink::Canvas)),
        (PickerField, f(Ink::Ink, Ink::Inset)),
        (PickerHit, f(Ink::Ink, Ink::Inset)),
        (PickerHitActive, f(Ink::Ink, Ink::Raised)),
        (EditEmpty, f(Ink::Faint, Ink::Canvas)),
        (EditCursor, f(Ink::Ink, Ink::Raised)),
        (Trajectory, f(Ink::Mute, Ink::Canvas)),
    ]
    .into_iter()
    .collect()
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

fn parse_ink(name: &str) -> Option<Ink> {
    serde_yaml::from_str(name).ok()
}

pub fn parse_color(s: &str) -> Color {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        }
    }
    match s.to_ascii_lowercase().as_str() {
        "reset" | "default" => Color::Reset,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "purple" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        _ => Color::Reset,
    }
}

static LIVE: OnceLock<Theme> = OnceLock::new();

pub fn install(theme: Theme) {
    let _ = LIVE.set(theme);
}

pub fn t() -> &'static Theme {
    LIVE.get_or_init(Theme::mocha)
}

pub fn p() -> &'static Theme {
    t()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_has_a_dotted_name_and_a_mocha_style() {
        let theme = Theme::mocha();
        let mut names = std::collections::BTreeSet::new();
        for face in Face::ALL {
            assert!(names.insert(face.as_str()), "duplicate {}", face.as_str());
            assert!(face
                .as_str()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '.'));
            assert!(
                theme.faces.contains_key(face),
                "mocha missing {}",
                face.as_str()
            );
            let _ = theme.style(*face);
        }
        assert_eq!(names.len(), Face::ALL.len());
    }

    #[test]
    fn spec_overrides_one_ink_and_one_face() {
        let spec = crate::config::ThemeConfig {
            pack: Some("mocha".into()),
            ink: BTreeMap::from([("accent".into(), "#ff00aa".into())]),
            face: BTreeMap::from([(
                "hint.key".into(),
                crate::config::ThemeFace {
                    fg: Some("accent".into()),
                    bg: None,
                    bold: Some(true),
                },
            )]),
        };
        let theme = Theme::from_config(&spec);
        assert_eq!(theme.color(Ink::Accent), Color::Rgb(255, 0, 170));
        let key = theme.faces.get(&Face::HintKey).unwrap();
        assert_eq!(key.fg, Ink::Accent);
        assert!(key.bold);
    }

    #[test]
    fn user_and_agent_fields_differ() {
        let theme = Theme::mocha();
        let user = theme.faces[&Face::MessageUserField];
        let agent = theme.faces[&Face::MessageAgentField];
        assert_ne!(user.bg, agent.bg);
    }
}
