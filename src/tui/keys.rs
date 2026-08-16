//! Configurable keymap. Defaults are herdr's map, then smith-only verbs
//! on chords herdr does not own.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    Detach,
    Help,
    Settings,
    ReloadConfig,
    Notify,
    ToggleRail,
    WorkspacePicker,
    Goto,
    NewWorkspace,
    NewWorktree,
    RenameWorkspace,
    CloseWorkspace,
    NewTab,
    RenameTab,
    CloseTab,
    NextSash,
    PrevSash,
    SwitchTab,
    FocusPaneLeft,
    FocusPaneDown,
    FocusPaneUp,
    FocusPaneRight,
    NavigatePaneLeft,
    NavigatePaneDown,
    NavigatePaneUp,
    NavigatePaneRight,
    SwapPaneLeft,
    SwapPaneDown,
    SwapPaneUp,
    SwapPaneRight,
    CyclePaneNext,
    CyclePanePrev,
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    Zoom,
    ResizeMode,
    CopyMode,
    RenamePane,
    EditScrollback,
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
    NewPlot,
    PageUp,
    PageDown,
    FocusCompose,
    ClearCompose,
    RailWorkspaceUp,
    RailWorkspaceDown,
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
        Action::Settings,
        Action::ReloadConfig,
        Action::Notify,
        Action::ToggleRail,
        Action::WorkspacePicker,
        Action::Goto,
        Action::NewWorkspace,
        Action::NewWorktree,
        Action::RenameWorkspace,
        Action::CloseWorkspace,
        Action::NewTab,
        Action::RenameTab,
        Action::CloseTab,
        Action::NextSash,
        Action::PrevSash,
        Action::SwitchTab,
        Action::FocusPaneLeft,
        Action::FocusPaneDown,
        Action::FocusPaneUp,
        Action::FocusPaneRight,
        Action::NavigatePaneLeft,
        Action::NavigatePaneDown,
        Action::NavigatePaneUp,
        Action::NavigatePaneRight,
        Action::SwapPaneLeft,
        Action::SwapPaneDown,
        Action::SwapPaneUp,
        Action::SwapPaneRight,
        Action::CyclePaneNext,
        Action::CyclePanePrev,
        Action::SplitVertical,
        Action::SplitHorizontal,
        Action::ClosePane,
        Action::Zoom,
        Action::ResizeMode,
        Action::CopyMode,
        Action::RenamePane,
        Action::EditScrollback,
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
        Action::NewPlot,
        Action::PageUp,
        Action::PageDown,
        Action::FocusCompose,
        Action::ClearCompose,
        Action::RailWorkspaceUp,
        Action::RailWorkspaceDown,
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
            Action::Settings => "settings",
            Action::ReloadConfig => "reload_config",
            Action::Notify => "open_notification_target",
            Action::ToggleRail => "toggle_sidebar",
            Action::WorkspacePicker => "workspace_picker",
            Action::Goto => "goto",
            Action::NewWorkspace => "new_workspace",
            Action::NewWorktree => "new_worktree",
            Action::RenameWorkspace => "rename_workspace",
            Action::CloseWorkspace => "close_workspace",
            Action::NewTab => "new_tab",
            Action::RenameTab => "rename_tab",
            Action::CloseTab => "close_tab",
            Action::NextSash => "next_tab",
            Action::PrevSash => "previous_tab",
            Action::SwitchTab => "switch_tab",
            Action::FocusPaneLeft => "focus_pane_left",
            Action::FocusPaneDown => "focus_pane_down",
            Action::FocusPaneUp => "focus_pane_up",
            Action::FocusPaneRight => "focus_pane_right",
            Action::NavigatePaneLeft => "navigate_pane_left",
            Action::NavigatePaneDown => "navigate_pane_down",
            Action::NavigatePaneUp => "navigate_pane_up",
            Action::NavigatePaneRight => "navigate_pane_right",
            Action::SwapPaneLeft => "swap_pane_left",
            Action::SwapPaneDown => "swap_pane_down",
            Action::SwapPaneUp => "swap_pane_up",
            Action::SwapPaneRight => "swap_pane_right",
            Action::CyclePaneNext => "cycle_pane_next",
            Action::CyclePanePrev => "cycle_pane_previous",
            Action::SplitVertical => "split_vertical",
            Action::SplitHorizontal => "split_horizontal",
            Action::ClosePane => "close_pane",
            Action::Zoom => "zoom",
            Action::ResizeMode => "resize_mode",
            Action::CopyMode => "copy_mode",
            Action::RenamePane => "rename_pane",
            Action::EditScrollback => "edit_scrollback",
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
            Action::NewPlot => "new_plot",
            Action::PageUp => "page_up",
            Action::PageDown => "page_down",
            Action::FocusCompose => "focus_compose",
            Action::ClearCompose => "clear_compose",
            Action::RailWorkspaceUp => "navigate_workspace_up",
            Action::RailWorkspaceDown => "navigate_workspace_down",
            Action::RailCycle => "rail_cycle",
            Action::RailEnter => "rail_enter",
            Action::PickerUp => "picker_up",
            Action::PickerDown => "picker_down",
            Action::PickerAccept => "picker_accept",
            Action::PickerCancel => "picker_cancel",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "next_sash" => Some(Action::NextSash),
            "prev_sash" | "previous_sash" => Some(Action::PrevSash),
            "toggle_rail" => Some(Action::ToggleRail),
            "rail_up" => Some(Action::NavigatePaneUp),
            "rail_down" => Some(Action::NavigatePaneDown),
            "rail_left" => Some(Action::NavigatePaneLeft),
            "rail_right" => Some(Action::NavigatePaneRight),
            "rail_workspace_up" => Some(Action::RailWorkspaceUp),
            "rail_workspace_down" => Some(Action::RailWorkspaceDown),
            other => Action::ALL.iter().copied().find(|a| a.as_str() == other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Detach => "detach",
            Action::Help => "keybind help",
            Action::Settings => "settings",
            Action::ReloadConfig => "reload config",
            Action::Notify => "notifications",
            Action::ToggleRail => "rail",
            Action::WorkspacePicker => "catalog picker",
            Action::Goto => "goto",
            Action::NewWorkspace => "new catalog",
            Action::NewWorktree => "new worktree",
            Action::RenameWorkspace => "rename catalog",
            Action::CloseWorkspace => "close catalog",
            Action::NewTab => "new sash",
            Action::RenameTab => "rename sash",
            Action::CloseTab => "close sash",
            Action::NextSash => "next sash",
            Action::PrevSash => "prev sash",
            Action::SwitchTab => "sash 1..9",
            Action::FocusPaneLeft => "pane left",
            Action::FocusPaneDown => "pane down",
            Action::FocusPaneUp => "pane up",
            Action::FocusPaneRight => "pane right",
            Action::NavigatePaneLeft => "nav pane left",
            Action::NavigatePaneDown => "nav pane down",
            Action::NavigatePaneUp => "nav pane up",
            Action::NavigatePaneRight => "nav pane right",
            Action::SwapPaneLeft => "swap pane left",
            Action::SwapPaneDown => "swap pane down",
            Action::SwapPaneUp => "swap pane up",
            Action::SwapPaneRight => "swap pane right",
            Action::CyclePaneNext => "cycle pane",
            Action::CyclePanePrev => "cycle pane prev",
            Action::SplitVertical => "split right",
            Action::SplitHorizontal => "split down",
            Action::ClosePane => "close pane",
            Action::Zoom => "zoom",
            Action::ResizeMode => "resize mode",
            Action::CopyMode => "copy mode",
            Action::RenamePane => "rename pane",
            Action::EditScrollback => "edit scrollback",
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
            Action::NewPlot => "new plot",
            Action::PageUp => "page up",
            Action::PageDown => "page down",
            Action::FocusCompose => "compose",
            Action::ClearCompose => "clear compose",
            Action::RailWorkspaceUp => "workspace up",
            Action::RailWorkspaceDown => "workspace down",
            Action::RailCycle => "rail cycle",
            Action::RailEnter => "rail enter",
            Action::PickerUp => "picker up",
            Action::PickerDown => "picker down",
            Action::PickerAccept => "picker accept",
            Action::PickerCancel => "picker cancel",
        }
    }

    pub fn canonical(self) -> Action {
        match self {
            Action::NavigatePaneLeft => Action::FocusPaneLeft,
            Action::NavigatePaneDown => Action::FocusPaneDown,
            Action::NavigatePaneUp => Action::FocusPaneUp,
            Action::NavigatePaneRight => Action::FocusPaneRight,
            Action::RailWorkspaceUp => Action::PrevSash,
            Action::RailWorkspaceDown => Action::NextSash,
            other => other,
        }
    }

    /// Where a direct (non-prefix) bind is allowed. Prefix binds are global.
    pub fn direct_ok(self, rail: bool, pty: bool, edit: bool, picker: bool, help: bool) -> bool {
        if help {
            return matches!(
                self,
                Action::Help
                    | Action::Settings
                    | Action::Detach
                    | Action::PickerCancel
                    | Action::FocusCompose
            );
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
                    | Action::FocusCompose
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
            Action::NavigatePaneLeft
                | Action::NavigatePaneDown
                | Action::NavigatePaneUp
                | Action::NavigatePaneRight
                | Action::RailWorkspaceUp
                | Action::RailWorkspaceDown
                | Action::RailCycle
                | Action::RailEnter
                | Action::FocusCompose
                | Action::NewSession
                | Action::NewPty
                | Action::NewEdit
                | Action::NewClock
                | Action::NewLog
                | Action::NewPlot
        ) {
            return rail;
        }
        if matches!(
            self,
            Action::Ask | Action::Strike | Action::Newline | Action::ClearCompose
        ) {
            return !pty && !edit && !rail;
        }
        if pty {
            return matches!(
                self,
                Action::Detach
                    | Action::Help
                    | Action::Settings
                    | Action::ReloadConfig
                    | Action::NextSash
                    | Action::PrevSash
                    | Action::SwitchTab
                    | Action::FocusPaneLeft
                    | Action::FocusPaneDown
                    | Action::FocusPaneUp
                    | Action::FocusPaneRight
                    | Action::SwapPaneLeft
                    | Action::SwapPaneDown
                    | Action::SwapPaneUp
                    | Action::SwapPaneRight
                    | Action::CyclePaneNext
                    | Action::CyclePanePrev
                    | Action::GrowPane
                    | Action::ShrinkPane
                    | Action::SplitVertical
                    | Action::SplitHorizontal
                    | Action::ClosePane
                    | Action::Zoom
                    | Action::ResizeMode
                    | Action::CopyMode
                    | Action::NewTab
                    | Action::CloseTab
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
    pub indexed: bool,
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
        if self.indexed {
            parts.push("1..9".into());
        } else {
            parts.push(key_name(self.code));
        }
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
    let mut indexed = false;
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
            "tab" => {
                if mods.contains(KeyModifiers::SHIFT) {
                    mods -= KeyModifiers::SHIFT;
                    key = Some(KeyCode::BackTab);
                } else {
                    key = Some(KeyCode::Tab);
                }
            }
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
            "1..9" | "1-9" => {
                indexed = true;
                key = Some(KeyCode::Char('1'));
            }
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
        indexed,
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
    if want.indexed {
        let KeyCode::Char(c) = code else {
            return false;
        };
        return c.is_ascii_digit() && c != '0' && mods == want.mods;
    }
    code == want.code && mods == want.mods
}

pub fn digit_of(key: KeyEvent) -> Option<u8> {
    match key.code {
        KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => c.to_digit(10).map(|d| d as u8),
        _ => None,
    }
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
                    km.binds.retain(|(oc, _)| *oc != c);
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

    #[allow(dead_code)]
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

    pub fn chords_of(&self, action: Action) -> String {
        let chords: Vec<String> = self
            .binds
            .iter()
            .filter(|(_, a)| *a == action)
            .map(|(c, _)| c.display())
            .collect();
        if chords.is_empty() {
            "unset".into()
        } else {
            chords.join(" / ")
        }
    }

    /// Herdr-shaped groups: heading then (keys, label) rows.
    pub fn help_groups(&self) -> Vec<(&'static str, Vec<(String, String)>)> {
        use Action::*;
        let row = |a: Action| (self.chords_of(a), a.label().into());
        vec![
            (
                "global",
                vec![
                    (self.prefix.display(), "prefix mode".into()),
                    row(Help),
                    row(Settings),
                    row(Detach),
                    row(ReloadConfig),
                    row(Notify),
                ],
            ),
            (
                "navigation",
                vec![
                    (self.chords_of(FocusCompose), "back".into()),
                    (
                        format!(
                            "{} / {}",
                            self.chords_of(RailWorkspaceUp),
                            self.chords_of(RailWorkspaceDown)
                        ),
                        "workspace list".into(),
                    ),
                    (
                        format!(
                            "{} / {} / {} / {}",
                            self.chords_of(NavigatePaneLeft),
                            self.chords_of(NavigatePaneDown),
                            self.chords_of(NavigatePaneUp),
                            self.chords_of(NavigatePaneRight)
                        ),
                        "move focus".into(),
                    ),
                    (self.chords_of(ToggleRail), "toggle rail".into()),
                    (self.chords_of(RailEnter), "open row".into()),
                    (self.chords_of(SwitchTab), "switch sash 1-9".into()),
                    (self.chords_of(RailCycle), "rail columns".into()),
                ],
            ),
            (
                "workspaces / tabs",
                vec![
                    row(WorkspacePicker),
                    row(Goto),
                    row(NewWorkspace),
                    row(NewWorktree),
                    row(RenameWorkspace),
                    row(CloseWorkspace),
                    row(NewTab),
                    row(RenameTab),
                    row(PrevSash),
                    row(NextSash),
                    row(CloseTab),
                ],
            ),
            (
                "panes",
                vec![
                    row(SplitVertical),
                    row(SplitHorizontal),
                    row(ClosePane),
                    row(RenamePane),
                    row(EditScrollback),
                    row(CopyMode),
                    row(Zoom),
                    row(ResizeMode),
                    row(GrowPane),
                    row(ShrinkPane),
                    row(ToggleRail),
                    row(FocusPaneLeft),
                    row(FocusPaneDown),
                    row(FocusPaneUp),
                    row(FocusPaneRight),
                    row(SwapPaneLeft),
                    row(SwapPaneDown),
                    row(SwapPaneUp),
                    row(SwapPaneRight),
                    row(CyclePaneNext),
                    row(CyclePanePrev),
                ],
            ),
            (
                "smith",
                vec![
                    row(Ask),
                    row(Strike),
                    row(Newline),
                    (
                        format!("esc esc / {}", self.chords_of(ClearCompose)),
                        "clear compose".into(),
                    ),
                    row(Fold),
                    row(Verbosity),
                    row(Mount),
                    row(Unmount),
                    row(Trajectory),
                    row(NewSession),
                    row(NewPty),
                    row(NewEdit),
                    row(NewClock),
                    row(NewLog),
                    row(NewPlot),
                    row(PageUp),
                    row(PageDown),
                ],
            ),
        ]
    }
}

pub fn filter_help_groups<'a>(
    groups: &'a [(&'static str, Vec<(String, String)>)],
    query: &str,
) -> Vec<(&'static str, Vec<(String, String)>)> {
    if query.is_empty() {
        return groups.to_vec();
    }
    let q = query.to_ascii_lowercase();
    groups
        .iter()
        .filter_map(|(name, rows)| {
            let rows: Vec<_> = rows
                .iter()
                .filter(|(k, l)| {
                    k.to_ascii_lowercase().contains(&q) || l.to_ascii_lowercase().contains(&q)
                })
                .cloned()
                .collect();
            (!rows.is_empty()).then_some((*name, rows))
        })
        .collect()
}

/// Herdr defaults first, then smith-only verbs on free chords.
const DEFAULTS: &[(&str, Action)] = &[
    ("prefix+q", Action::Detach),
    ("prefix+?", Action::Help),
    ("prefix+s", Action::Settings),
    ("prefix+shift+r", Action::ReloadConfig),
    ("prefix+o", Action::Notify),
    ("prefix+b,tab", Action::ToggleRail),
    ("prefix+w", Action::WorkspacePicker),
    ("prefix+g", Action::Goto),
    ("prefix+shift+n", Action::NewWorkspace),
    ("prefix+shift+g", Action::NewWorktree),
    ("prefix+shift+w", Action::RenameWorkspace),
    ("prefix+shift+d", Action::CloseWorkspace),
    ("prefix+c,ctrl+alt+c", Action::NewTab),
    ("prefix+shift+t", Action::RenameTab),
    ("prefix+shift+x", Action::CloseTab),
    ("prefix+n,ctrl+alt+]", Action::NextSash),
    ("prefix+p,ctrl+alt+[", Action::PrevSash),
    ("prefix+1..9", Action::SwitchTab),
    ("prefix+h,ctrl+alt+h", Action::FocusPaneLeft),
    ("prefix+j,ctrl+alt+j", Action::FocusPaneDown),
    ("prefix+k,ctrl+alt+k", Action::FocusPaneUp),
    ("prefix+l,ctrl+alt+l", Action::FocusPaneRight),
    ("h,left", Action::NavigatePaneLeft),
    ("j", Action::NavigatePaneDown),
    ("k", Action::NavigatePaneUp),
    ("l,right", Action::NavigatePaneRight),
    ("prefix+shift+h", Action::SwapPaneLeft),
    ("prefix+shift+j", Action::SwapPaneDown),
    ("prefix+shift+k", Action::SwapPaneUp),
    ("prefix+shift+l", Action::SwapPaneRight),
    ("prefix+tab", Action::CyclePaneNext),
    ("prefix+shift+tab", Action::CyclePanePrev),
    ("prefix+v,ctrl+alt+d", Action::SplitVertical),
    ("prefix+minus,ctrl+alt+shift+d", Action::SplitHorizontal),
    ("prefix+x", Action::ClosePane),
    ("prefix+z,ctrl+alt+z", Action::Zoom),
    ("prefix+r", Action::ResizeMode),
    ("prefix+[", Action::CopyMode),
    ("prefix+shift+p", Action::RenamePane),
    ("prefix+e", Action::EditScrollback),
    ("prefix+plus,prefix+=", Action::GrowPane),
    ("prefix+shift+minus", Action::ShrinkPane),
    ("enter", Action::Ask),
    ("ctrl+s,prefix+enter", Action::Strike),
    ("shift+enter,ctrl+j", Action::Newline),
    ("ctrl+u", Action::ClearCompose),
    ("prefix+.", Action::Fold),
    ("prefix+shift+v", Action::Verbosity),
    ("prefix+m", Action::Mount),
    ("prefix+u", Action::Unmount),
    ("prefix+shift+y", Action::Trajectory),
    ("n", Action::NewSession),
    ("prefix+t,p", Action::NewPty),
    ("e", Action::NewEdit),
    ("prefix+shift+c,c", Action::NewClock),
    ("g", Action::NewLog),
    ("o", Action::NewPlot),
    ("pageup", Action::PageUp),
    ("pagedown", Action::PageDown),
    ("esc", Action::FocusCompose),
    ("up", Action::RailWorkspaceUp),
    ("down", Action::RailWorkspaceDown),
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
        let ix = parse_chord("prefix+1..9").unwrap();
        assert!(ix.prefix && ix.indexed);
        let tab = parse_chord("prefix+shift+tab").unwrap();
        assert!(tab.prefix);
        assert_eq!(tab.code, KeyCode::BackTab);
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
    fn herdr_first_five() {
        let km = Keymap::defaults();
        let none = KeyModifiers::NONE;
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('c'), none), true, |_| true),
            Some(Action::NewTab)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('v'), none), true, |_| true),
            Some(Action::SplitVertical)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('-'), none), true, |_| true),
            Some(Action::SplitHorizontal)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('h'), none), true, |_| true),
            Some(Action::FocusPaneLeft)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('w'), none), true, |_| true),
            Some(Action::WorkspacePicker)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('q'), none), true, |_| true),
            Some(Action::Detach)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('x'), none), true, |_| true),
            Some(Action::ClosePane)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('z'), none), true, |_| true),
            Some(Action::Zoom)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('s'), none), true, |_| true),
            Some(Action::Settings)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Char('3'), none), true, |_| true),
            Some(Action::SwitchTab)
        );
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
        assert!(Action::NavigatePaneDown.direct_ok(true, false, false, false, false));
        assert!(!Action::NavigatePaneDown.direct_ok(false, false, false, false, false));
        assert!(Action::RailWorkspaceUp.direct_ok(true, false, false, false, false));
        assert!(!Action::RailWorkspaceUp.direct_ok(false, false, false, false, false));
        assert!(!Action::Ask.direct_ok(false, true, false, false, false));
        assert!(Action::Detach.direct_ok(false, true, false, false, false));
        assert!(Action::FocusPaneDown.direct_ok(false, true, false, false, false));
        assert!(!Action::NavigatePaneDown.direct_ok(false, true, false, false, false));
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

    #[test]
    fn smith_verbs_left_herdr_chords() {
        let km = Keymap::defaults();
        let none = KeyModifiers::NONE;
        assert_ne!(
            km.resolve(KeyEvent::new(KeyCode::Char('s'), none), true, |_| true),
            Some(Action::Strike)
        );
        assert_ne!(
            km.resolve(KeyEvent::new(KeyCode::Char('c'), none), true, |_| true),
            Some(Action::NewSession)
        );
        assert_ne!(
            km.resolve(KeyEvent::new(KeyCode::Char('e'), none), true, |_| true),
            Some(Action::NewEdit)
        );
        assert_eq!(
            km.resolve(KeyEvent::new(KeyCode::Enter, none), true, |_| true),
            Some(Action::Strike)
        );
        assert_eq!(
            km.resolve(
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
                false,
                |_| true
            ),
            Some(Action::ClearCompose)
        );
    }

    #[test]
    fn help_groups_filter_by_key_or_label() {
        let km = Keymap::defaults();
        let groups = km.help_groups();
        assert!(groups.iter().any(|(n, _)| *n == "global"));
        assert!(groups.iter().any(|(n, _)| *n == "panes"));
        let hit = filter_help_groups(&groups, "split");
        assert!(hit.iter().all(|(n, rows)| {
            *n == "panes"
                && rows
                    .iter()
                    .any(|(k, l)| k.contains("prefix+v") || l.contains("split"))
        }));
        assert!(filter_help_groups(&groups, "zzzz-nope").is_empty());
    }
}
