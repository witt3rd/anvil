//! Configurable keymap. Herdr's model: a prefix so we do not steal
//! keys from a focused PTY/editor, plus optional direct chords.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    Detach,
    Help,
    ToggleRail,
    NextSash,
    PrevSash,
    NextPane,
    PrevPane,
    GrowPane,
    ShrinkPane,
    Ask,
    Strike,
    Newline,
    Fold,
    Verbosity,
    Mount,
    Unmount,
    Trajectory,
    NewSession,
    NewPty,
    NewEdit,
    NewClock,
    NewLog,
    PageUp,
    PageDown,
    FocusCompose,
    RailUp,
    RailDown,
    RailLeft,
    RailRight,
    RailCycle,
    RailEnter,
    PickerUp,
    PickerDown,
    PickerAccept,
    PickerCancel,
}

impl Action {
    pub const ALL: &'static [Action] = &[
        Action::Detach,
        Action::Help,
        Action::ToggleRail,
        Action::NextSash,
        Action::PrevSash,
        Action::NextPane,
        Action::PrevPane,
        Action::GrowPane,
        Action::ShrinkPane,
        Action::Ask,
        Action::Strike,
        Action::Newline,
        Action::Fold,
        Action::Verbosity,
        Action::Mount,
        Action::Unmount,
        Action::Trajectory,
        Action::NewSession,
        Action::NewPty,
        Action::NewEdit,
        Action::NewClock,
        Action::NewLog,
        Action::PageUp,
        Action::PageDown,
        Action::FocusCompose,
        Action::RailUp,
        Action::RailDown,
        Action::RailLeft,
        Action::RailRight,
        Action::RailCycle,
        Action::RailEnter,
        Action::PickerUp,
        Action::PickerDown,
        Action::PickerAccept,
        Action::PickerCancel,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Action::Detach => "detach",
            Action::Help => "help",
            Action::ToggleRail => "toggle_rail",
            Action::NextSash => "next_sash",
            Action::PrevSash => "prev_sash",
            Action::NextPane => "next_pane",
            Action::PrevPane => "prev_pane",
            Action::GrowPane => "grow_pane",
            Action::ShrinkPane => "shrink_pane",
            Action::Ask => "ask",
            Action::Strike => "strike",
            Action::Newline => "newline",
            Action::Fold => "fold",
            Action::Verbosity => "verbosity",
            Action::Mount => "mount",
            Action::Unmount => "unmount",
            Action::Trajectory => "trajectory",
            Action::NewSession => "new_session",
            Action::NewPty => "new_pty",
            Action::NewEdit => "new_edit",
            Action::NewClock => "new_clock",
            Action::NewLog => "new_log",
            Action::PageUp => "page_up",
            Action::PageDown => "page_down",
            Action::FocusCompose => "focus_compose",
            Action::RailUp => "rail_up",
            Action::RailDown => "rail_down",
            Action::RailLeft => "rail_left",
            Action::RailRight => "rail_right",
            Action::RailCycle => "rail_cycle",
            Action::RailEnter => "rail_enter",
            Action::PickerUp => "picker_up",
            Action::PickerDown => "picker_down",
            Action::PickerAccept => "picker_accept",
            Action::PickerCancel => "picker_cancel",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Action::ALL.iter().copied().find(|a| a.as_str() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Detach => "close casing",
            Action::Help => "keybind help",
            Action::ToggleRail => "rail",
            Action::NextSash => "next sash",
            Action::PrevSash => "prev sash",
            Action::NextPane => "next pane",
            Action::PrevPane => "prev pane",
            Action::GrowPane => "grow pane",
            Action::ShrinkPane => "shrink pane",
            Action::Ask => "ask",
            Action::Strike => "strike",
            Action::Newline => "newline",
            Action::Fold => "fold",
            Action::Verbosity => "verbosity",
            Action::Mount => "mount clock",
            Action::Unmount => "unmount",
            Action::Trajectory => "trajectory",
            Action::NewSession => "new session",
            Action::NewPty => "new pty",
            Action::NewEdit => "new edit",
            Action::NewClock => "new clock",
            Action::NewLog => "new log",
            Action::PageUp => "page up",
            Action::PageDown => "page down",
            Action::FocusCompose => "compose",
            Action::RailUp => "rail up",
            Action::RailDown => "rail down",
            Action::RailLeft => "rail left",
            Action::RailRight => "rail right",
            Action::RailCycle => "rail cycle",
            Action::RailEnter => "rail enter",
            Action::PickerUp => "picker up",
            Action::PickerDown => "picker down",
            Action::PickerAccept => "picker accept",
            Action::PickerCancel => "picker cancel",
        }
    }

    /// Where a direct (non-prefix) bind is allowed. Prefix binds are global.
    pub fn direct_ok(self, rail: bool, pty: bool, edit: bool, picker: bool, help: bool) -> bool {
        if help {
            return matches!(self, Action::Help | Action::Detach | Action::PickerCancel);
        }
        if picker {
            return matches!(
                self,
                Action::PickerUp
                    | Action::PickerDown
                    | Action::PickerAccept
                    | Action::PickerCancel
                    | Action::Detach
                    | Action::Help
            );
        }
        if matches!(
            self,
            Action::PickerUp | Action::PickerDown | Action::PickerAccept | Action::PickerCancel
        ) {
            return false;
        }
        if matches!(
            self,
            Action::RailUp
                | Action::RailDown
                | Action::RailLeft
                | Action::RailRight
                | Action::RailCycle
                | Action::RailEnter
                | Action::FocusCompose
                | Action::NewSession
                | Action::NewPty
                | Action::NewEdit
                | Action::NewClock
                | Action::NewLog
        ) {
            return rail;
        }
        if matches!(self, Action::Ask | Action::Strike | Action::Newline) {
            return !pty && !edit && !rail;
        }
        if pty {
            return matches!(
                self,
                Action::Detach
                    | Action::Help
                    | Action::NextSash
                    | Action::PrevSash
                    | Action::NextPane
                    | Action::PrevPane
                    | Action::GrowPane
                    | Action::ShrinkPane
                    | Action::Mount
                    | Action::Unmount
                    | Action::Trajectory
            );
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub prefix: bool,
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Chord {
    pub fn display(self) -> String {
        let mut parts = Vec::new();
        if self.prefix {
            parts.push("prefix".into());
        }
        if self.mods.contains(KeyModifiers::CONTROL) {
            parts.push("ctrl".into());
        }
        if self.mods.contains(KeyModifiers::ALT) {
            parts.push("alt".into());
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            parts.push("shift".into());
        }
        if self.mods.contains(KeyModifiers::SUPER) {
            parts.push("cmd".into());
        }
        parts.push(key_name(self.code));
        parts.join("+")
    }
}

fn key_name(code: KeyCode) -> String {
    match code {
        KeyCode::Enter => "enter".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "shift+tab".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char('-') => "minus".into(),
        KeyCode::Char('+') => "plus".into(),
        KeyCode::Char('?') => "?".into(),
        KeyCode::Char(c) => c.to_string(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub fn parse_chord(raw: &str) -> Option<Chord> {
    let mut prefix = false;
    let mut mods = KeyModifiers::NONE;
    let mut key: Option<KeyCode> = None;
    for part in raw.split('+') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        match p.to_ascii_lowercase().as_str() {
            "prefix" => prefix = true,
            "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
            "alt" | "opt" | "option" => mods |= KeyModifiers::ALT,
            "shift" => mods |= KeyModifiers::SHIFT,
            "cmd" | "super" | "win" => mods |= KeyModifiers::SUPER,
            "enter" | "return" => key = Some(KeyCode::Enter),
            "tab" => key = Some(KeyCode::Tab),
            "esc" | "escape" => key = Some(KeyCode::Esc),
            "backspace" | "bs" => key = Some(KeyCode::Backspace),
            "delete" | "del" => key = Some(KeyCode::Delete),
            "left" => key = Some(KeyCode::Left),
            "right" => key = Some(KeyCode::Right),
            "up" => key = Some(KeyCode::Up),
            "down" => key = Some(KeyCode::Down),
            "home" => key = Some(KeyCode::Home),
            "end" => key = Some(KeyCode::End),
            "pageup" | "pgup" => key = Some(KeyCode::PageUp),
            "pagedown" | "pgdn" => key = Some(KeyCode::PageDown),
            "space" => key = Some(KeyCode::Char(' ')),
            "minus" | "hyphen" | "dash" => key = Some(KeyCode::Char('-')),
            "plus" => key = Some(KeyCode::Char('+')),
            "comma" => key = Some(KeyCode::Char(',')),
            "period" | "dot" => key = Some(KeyCode::Char('.')),
            "slash" => key = Some(KeyCode::Char('/')),
            "question" => key = Some(KeyCode::Char('?')),
            "backtick" => key = Some(KeyCode::Char('`')),
            "[" => key = Some(KeyCode::Char('[')),
            "]" => key = Some(KeyCode::Char(']')),
            "=" => key = Some(KeyCode::Char('=')),
            other if other.chars().count() == 1 => {
                let c = other.chars().next()?;
                key = Some(KeyCode::Char(c));
            }
            _ => return None,
        }
    }
    Some(Chord {
        prefix,
        code: key?,
        mods,
    })
}

fn key_eq(got: KeyEvent, want: Chord) -> bool {
    let mut code = got.code;
    let mut mods = got.modifiers - KeyModifiers::NONE;
    if let KeyCode::Char(c) = code {
        if c.is_ascii_uppercase() {
            code = KeyCode::Char(c.to_ascii_lowercase());
            mods |= KeyModifiers::SHIFT;
        }
    }
    if matches!(code, KeyCode::Char('?')) {
        mods = mods - KeyModifiers::SHIFT;
    }
    if want.code == KeyCode::Char('?')
        && matches!(got.code, KeyCode::Char('/') | KeyCode::Char('?'))
        && (got.modifiers.contains(KeyModifiers::SHIFT) || matches!(got.code, KeyCode::Char('?')))
    {
        return (mods - KeyModifiers::SHIFT) == (want.mods - KeyModifiers::SHIFT);
    }
    code == want.code && mods == want.mods
}

#[derive(Debug, Clone)]
pub struct Keymap {
    pub prefix: Chord,
    binds: Vec<(Chord, Action)>,
}

impl Keymap {
    pub fn defaults() -> Self {
        let mut km = Self {
            prefix: parse_chord("ctrl+b").unwrap(),
            binds: Vec::new(),
        };
        for (spec, action) in DEFAULTS {
            for s in spec.split(',') {
                if let Some(c) = parse_chord(s.trim()) {
                    km.binds.push((c, *action));
                }
            }
        }
        km
    }

    pub fn from_config(cfg: &crate::config::KeysConfig) -> Self {
        let mut km = Self::defaults();
        if let Some(p) = cfg.prefix.as_deref().and_then(parse_chord) {
            km.prefix = p;
        }
        for (name, spec) in &cfg.actions {
            let Some(action) = Action::parse(name) else {
                continue;
            };
            km.binds.retain(|(_, a)| *a != action);
            for raw in spec.as_slice() {
                if let Some(c) = parse_chord(raw) {
                    km.binds.push((c, action));
                }
            }
        }
        km
    }

    pub fn is_prefix(&self, key: KeyEvent) -> bool {
        !self.prefix.prefix && key_eq(key, self.prefix)
    }

    pub fn resolve(
        &self,
        key: KeyEvent,
        after_prefix: bool,
        ok: impl Fn(Action) -> bool,
    ) -> Option<Action> {
        self.binds.iter().find_map(|(c, a)| {
            if c.prefix != after_prefix {
                return None;
            }
            if !key_eq(key, *c) || !ok(*a) {
                return None;
            }
            Some(*a)
        })
    }

    pub fn display(&self, action: Action) -> String {
        self.binds
            .iter()
            .find(|(_, a)| *a == action)
            .map(|(c, _)| c.display())
            .unwrap_or_else(|| action.as_str().into())
    }

    pub fn help(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        out.push(("prefix".into(), self.prefix.display()));
        for action in Action::ALL {
            let chords: Vec<String> = self
                .binds
                .iter()
                .filter(|(_, a)| a == action)
                .map(|(c, _)| c.display())
                .collect();
            if !chords.is_empty() {
                out.push((action.label().into(), chords.join(", ")));
            }
        }
        out
    }
}

/// Herdr-shaped defaults: prefix-first, ctrl+alt as the safe direct family.
const DEFAULTS: &[(&str, Action)] = &[
    ("prefix+q", Action::Detach),
    ("prefix+?", Action::Help),
    ("prefix+b,tab", Action::ToggleRail),
    ("prefix+n,ctrl+alt+]", Action::NextSash),
    ("prefix+p,ctrl+alt+[", Action::PrevSash),
    ("prefix+j,ctrl+alt+j", Action::NextPane),
    ("prefix+k,ctrl+alt+k", Action::PrevPane),
    ("prefix+plus,prefix+=", Action::GrowPane),
    ("prefix+minus", Action::ShrinkPane),
    ("enter", Action::Ask),
    ("prefix+s,ctrl+s", Action::Strike),
    ("shift+enter,ctrl+j", Action::Newline),
    ("prefix+.", Action::Fold),
    ("prefix+shift+v", Action::Verbosity),
    ("prefix+m", Action::Mount),
    ("prefix+u", Action::Unmount),
    ("prefix+shift+l", Action::Trajectory),
    ("prefix+c,n", Action::NewSession),
    ("prefix+t,p", Action::NewPty),
    ("prefix+e,e", Action::NewEdit),
    ("prefix+shift+c,c", Action::NewClock),
    ("prefix+shift+g,g", Action::NewLog),
    ("pageup", Action::PageUp),
    ("pagedown", Action::PageDown),
    ("esc", Action::FocusCompose),
    ("up,k", Action::RailUp),
    ("down,j", Action::RailDown),
    ("left,h", Action::RailLeft),
    ("right,l", Action::RailRight),
    ("[", Action::RailCycle),
    ("enter", Action::RailEnter),
    ("up", Action::PickerUp),
    ("down", Action::PickerDown),
    ("enter,tab", Action::PickerAccept),
    ("esc", Action::PickerCancel),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parse_prefix_and_direct() {
        let p = parse_chord("prefix+n").unwrap();
        assert!(p.prefix);
        assert_eq!(p.code, KeyCode::Char('n'));
        let d = parse_chord("ctrl+alt+]").unwrap();
        assert!(!d.prefix);
        assert!(d.mods.contains(KeyModifiers::CONTROL | KeyModifiers::ALT));
        assert_eq!(d.code, KeyCode::Char(']'));
        assert!(parse_chord("prefix+minus").unwrap().code == KeyCode::Char('-'));
    }

    #[test]
    fn defaults_are_herdr_shaped() {
        let km = Keymap::defaults();
        assert!(km.is_prefix(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)));
        let n = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(km.resolve(n, true, |_| true), Some(Action::NextSash));
        let chord = KeyEvent::new(
            KeyCode::Char(']'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_eq!(km.resolve(chord, false, |_| true), Some(Action::NextSash));
        let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(km.resolve(q, true, |_| true), Some(Action::Detach));
    }

    #[test]
    fn config_replaces_one_action() {
        let mut actions = BTreeMap::new();
        actions.insert(
            "detach".into(),
            crate::config::KeySpec::One("prefix+x".into()),
        );
        let cfg = crate::config::KeysConfig {
            prefix: Some("ctrl+a".into()),
            actions,
        };
        let km = Keymap::from_config(&cfg);
        assert!(km.is_prefix(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)));
        assert_eq!(
            km.resolve(
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
                true,
                |_| true
            ),
            Some(Action::Detach)
        );
        assert_eq!(
            km.resolve(
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
                true,
                |_| true
            ),
            None
        );
    }

    #[test]
    fn rail_plain_keys_are_not_global() {
        assert!(Action::RailUp.direct_ok(true, false, false, false, false));
        assert!(!Action::RailUp.direct_ok(false, false, false, false, false));
        assert!(!Action::Ask.direct_ok(false, true, false, false, false));
        assert!(Action::Detach.direct_ok(false, true, false, false, false));
    }

    #[test]
    fn every_action_has_a_default_bind() {
        let km = Keymap::defaults();
        for a in Action::ALL {
            assert!(
                km.binds.iter().any(|(_, x)| x == a),
                "missing default for {}",
                a.as_str()
            );
        }
    }
}
