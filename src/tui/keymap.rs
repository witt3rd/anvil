use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_which_key::{Keymap, WhichKeyState};

const PREFIX_KEY: char = 'b';

/// Every action is a documented op on the wire (`docs/protocol.md`).
/// An action that has no op has no place here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    Detach,
    Help,
    NewSession,
    SwitchSession(u8),
    NewWindow,
    SplitVertical,
    SplitHorizontal,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Detach => write!(f, "detach"),
            Action::Help => write!(f, "help"),
            Action::NewSession => write!(f, "new session"),
            Action::SwitchSession(n) => write!(f, "session {n}"),
            Action::NewWindow => write!(f, "new window"),
            Action::SplitVertical => write!(f, "split right"),
            Action::SplitHorizontal => write!(f, "split down"),
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
        for n in 1..=9 {
            s.bind(&n.to_string(), Action::SwitchSession(n), Category::Session);
        }

        // window
        s.bind("c", Action::NewWindow, Category::Window);

        // pane
        s.bind("v", Action::SplitVertical, Category::Pane);
        s.bind("-", Action::SplitHorizontal, Category::Pane);
    });

    km
}

pub fn build_which_key_state() -> AppWhichKey {
    WhichKeyState::new(build_keymap(), Scope::Global)
}