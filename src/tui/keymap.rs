use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_which_key::{Keymap, WhichKeyState};

const PREFIX_KEY: char = 'b';

/// Actions are documented wire ops (`docs/protocol.md`), plus the
/// client's own chrome (help, the rail/roster toggle).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    Detach,
    Help,
    ToggleRoster,
    RenameWindow,
    NewSession,
    SwitchSession(u8),
    NewWindow,
    NewAgent,
    PickAgent,
    NextWindow,
    PrevWindow,
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    CloseWindow,
    FocusLeft,
    FocusDown,
    FocusUp,
    FocusRight,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Detach => write!(f, "detach"),
            Action::Help => write!(f, "help"),
            Action::ToggleRoster => write!(f, "roster"),
            Action::RenameWindow => write!(f, "rename window"),
            Action::NewSession => write!(f, "new session"),
            Action::SwitchSession(n) => write!(f, "session {n}"),
            Action::NewWindow => write!(f, "new window"),
            Action::NewAgent => write!(f, "new agent"),
            Action::PickAgent => write!(f, "pick agent"),
            Action::NextWindow => write!(f, "next window"),
            Action::PrevWindow => write!(f, "previous window"),
            Action::SplitVertical => write!(f, "split right"),
            Action::SplitHorizontal => write!(f, "split down"),
            Action::ClosePane => write!(f, "close pane"),
            Action::CloseWindow => write!(f, "close window"),
            Action::FocusLeft => write!(f, "focus left"),
            Action::FocusDown => write!(f, "focus down"),
            Action::FocusUp => write!(f, "focus up"),
            Action::FocusRight => write!(f, "focus right"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    Global,
    Prefix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
pub enum Category {
    Session,
    Window,
    Pane,
}

pub type AppKeymap = Keymap<KeyEvent, Scope, Action, Category>;
pub type AppWhichKey = WhichKeyState<KeyEvent, Scope, Action, Category>;

pub fn prefix_key_event() -> KeyEvent {
    KeyEvent::new(KeyCode::Char(PREFIX_KEY), KeyModifiers::CONTROL)
}

pub fn build_keymap() -> AppKeymap {
    let mut km = Keymap::new();

    km.scope(Scope::Prefix, |s| {
        s.describe_group("", "prefix");

        // session
        s.bind("q", Action::Detach, Category::Session);
        s.bind("?", Action::Help, Category::Session);
        s.bind("n", Action::NewSession, Category::Session);
        s.bind("s", Action::ToggleRoster, Category::Session);
        s.bind(",", Action::RenameWindow, Category::Window);
        s.bind("r", Action::RenameWindow, Category::Window);
        for n in 1..=9 {
            s.bind(&n.to_string(), Action::SwitchSession(n), Category::Session);
        }

        // window
        s.bind("c", Action::NewWindow, Category::Window);
        s.bind("a", Action::NewAgent, Category::Window);
        s.bind("A", Action::PickAgent, Category::Window);
        s.bind("]", Action::NextWindow, Category::Window);
        s.bind("[", Action::PrevWindow, Category::Window);

        // pane
        s.bind("v", Action::SplitVertical, Category::Pane);
        s.bind("-", Action::SplitHorizontal, Category::Pane);
        s.bind("x", Action::ClosePane, Category::Pane);
        s.bind("h", Action::FocusLeft, Category::Pane);
        s.bind("j", Action::FocusDown, Category::Pane);
        s.bind("k", Action::FocusUp, Category::Pane);
        s.bind("l", Action::FocusRight, Category::Pane);
        s.bind("<Left>", Action::FocusLeft, Category::Pane);
        s.bind("<Down>", Action::FocusDown, Category::Pane);
        s.bind("<Up>", Action::FocusUp, Category::Pane);
        s.bind("<Right>", Action::FocusRight, Category::Pane);
        s.bind("w", Action::CloseWindow, Category::Window);
    });

    km
}

pub fn build_which_key_state() -> AppWhichKey {
    WhichKeyState::new(build_keymap(), Scope::Global)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn prefixed() -> AppWhichKey {
        let mut wk = build_which_key_state();
        wk.set_scope(Scope::Prefix);
        wk.toggle();
        wk
    }

    #[test]
    fn arrows_dispatch_focus() {
        let cases = [
            (KeyCode::Right, Action::FocusRight),
            (KeyCode::Left, Action::FocusLeft),
            (KeyCode::Up, Action::FocusUp),
            (KeyCode::Down, Action::FocusDown),
        ];
        for (code, want) in cases {
            let mut wk = prefixed();
            let key = KeyEvent::new(code, KeyModifiers::NONE);
            assert_eq!(wk.handle_key(key), Some(want), "arrow {code:?}");
        }
    }

    #[test]
    fn comma_and_r_rename_the_window() {
        for c in [',', 'r'] {
            let mut wk = prefixed();
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert_eq!(wk.handle_key(key), Some(Action::RenameWindow), "key {c}");
        }
    }

    #[test]
    fn a_c_and_shift_a_launch() {
        let mut wk = prefixed();
        let a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(wk.handle_key(a), Some(Action::NewAgent));
        let mut wk = prefixed();
        let shift_a = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(wk.handle_key(shift_a), Some(Action::PickAgent));
        let mut wk = prefixed();
        let c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        assert_eq!(wk.handle_key(c), Some(Action::NewWindow));
        let mut wk = prefixed();
        let t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(wk.handle_key(t), None);
    }

    #[test]
    fn s_toggles_roster() {
        let mut wk = prefixed();
        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(wk.handle_key(key), Some(Action::ToggleRoster));
    }

    #[test]
    fn hjkl_dispatch_focus() {
        let cases = [
            ('h', Action::FocusLeft),
            ('j', Action::FocusDown),
            ('k', Action::FocusUp),
            ('l', Action::FocusRight),
        ];
        for (c, want) in cases {
            let mut wk = prefixed();
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert_eq!(wk.handle_key(key), Some(want), "key {c}");
        }
    }
}