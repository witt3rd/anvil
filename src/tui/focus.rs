//! Whether this client is the user's focused application.

use std::process::Command;

/// Bell when a turn ends unless we are looking at that pane.
pub fn should_bell(app_active: bool, pane_selected: bool) -> bool {
    !(app_active && pane_selected)
}

/// Hyprland's focused window owns this process, or the terminal said
/// it still has focus.
pub fn app_is_active(term_focused: bool) -> bool {
    hyprland_holds_us().unwrap_or(term_focused)
}

fn hyprland_holds_us() -> Option<bool> {
    let out = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let pid = parse_active_pid(&out.stdout)?;
    Some(process_is_under(pid))
}

fn parse_active_pid(bytes: &[u8]) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    v.get("pid")?.as_u64().map(|n| n as u32)
}

fn process_is_under(window_pid: u32) -> bool {
    let mut pid = std::process::id();
    for _ in 0..64 {
        if pid == window_pid {
            return true;
        }
        if pid <= 1 {
            return false;
        }
        pid = match ppid(pid) {
            Some(p) => p,
            None => return false,
        };
    }
    false
}

fn ppid(pid: u32) -> Option<u32> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit_once(')')?.1;
    rest.split_whitespace().nth(1)?.parse().ok()
}

pub fn bell() {
    use std::io::Write;
    let _ = std::io::stdout().write_all(b"\x07");
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bell_only_skips_when_looking_at_that_pane() {
        assert!(!should_bell(true, true));
        assert!(should_bell(true, false));
        assert!(should_bell(false, true));
        assert!(should_bell(false, false));
    }

    #[test]
    fn we_are_under_our_own_pid() {
        assert!(process_is_under(std::process::id()));
        assert!(!process_is_under(0));
    }

    #[test]
    fn hyprland_pid_parses() {
        assert_eq!(parse_active_pid(br#"{"pid": 4321, "class": "ghostty"}"#), Some(4321));
        assert_eq!(parse_active_pid(b"not json"), None);
    }
}
