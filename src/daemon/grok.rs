//! Grok's on-disk session: title and whether a turn is in flight.
//! The TUI has no HTTP door; the files under `~/.grok` are the watch.

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use super::adopt;

/// Terse activity for the grok process tree rooted at `pid`.
pub fn activity(pid: u32) -> Option<String> {
    let hit = live(pid)?;
    let summary = read_summary(&hit)?;
    if let Some(title) = summary.title().filter(|t| !placeholder(t)) {
        return Some(terse(title, 28));
    }
    last_user(&hit).map(|s| terse(&s, 28))
}

/// A grok child running `systemd-inhibit … turn in progress`.
pub fn turning(pid: u32) -> bool {
    if let Some(hit) = live(pid) {
        if tree_has_inhibit(hit.pid) {
            return true;
        }
    }
    tree_has_inhibit(pid)
}

fn tree_has_inhibit(root: u32) -> bool {
    let mut pids = vec![root];
    pids.extend(adopt::descendants(root));
    pids.iter().any(|p| {
        let cmd = cmdline(*p);
        cmd.contains("systemd-inhibit") && cmd.contains("turn in progress")
    })
}

#[derive(Debug, Clone)]
struct Hit {
    session_id: String,
    cwd: String,
    pid: u32,
}

#[derive(Debug, Deserialize)]
struct Active {
    session_id: String,
    pid: u32,
    cwd: String,
}

#[derive(Debug, Deserialize)]
struct Summary {
    #[serde(default)]
    generated_title: Option<String>,
    #[serde(default)]
    session_summary: Option<String>,
}

impl Summary {
    fn title(&self) -> Option<&str> {
        self.generated_title
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(self.session_summary.as_deref().filter(|s| !s.is_empty()))
    }
}

fn live(root: u32) -> Option<Hit> {
    let active: Vec<Active> =
        serde_json::from_str(&fs::read_to_string(grok_home().join("active_sessions.json")).ok()?)
            .ok()?;
    for a in &active {
        if a.pid == root || ancestor_of(a.pid, root) {
            return Some(Hit {
                session_id: a.session_id.clone(),
                cwd: a.cwd.clone(),
                pid: a.pid,
            });
        }
    }
    let down: Vec<u32> = std::iter::once(root)
        .chain(adopt::descendants(root))
        .collect();
    active.into_iter().find(|a| down.contains(&a.pid)).map(|a| Hit {
        session_id: a.session_id,
        cwd: a.cwd,
        pid: a.pid,
    })
}

/// Walk `/proc` parents from `child` up to `ancestor`.
fn ancestor_of(child: u32, ancestor: u32) -> bool {
    let mut p = child;
    for _ in 0..64 {
        if p == ancestor {
            return true;
        }
        match ppid(p) {
            Some(pp) if pp > 1 && pp != p => p = pp,
            _ => return false,
        }
    }
    false
}

fn ppid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().nth(1)?.parse().ok()
}

fn read_summary(hit: &Hit) -> Option<Summary> {
    let text = fs::read_to_string(session_dir(&hit.cwd).join(&hit.session_id).join("summary.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn last_user(hit: &Hit) -> Option<String> {
    let path = session_dir(&hit.cwd).join(&hit.session_id).join("chat_history.jsonl");
    let text = fs::read_to_string(path).ok()?;
    let mut last = None;
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        if let Some(s) = user_text(&v) {
            last = Some(s);
            break;
        }
    }
    last
}

fn user_text(v: &serde_json::Value) -> Option<String> {
    let content = v.get("content")?;
    let raw = if let Some(s) = content.as_str() {
        s.to_string()
    } else if let Some(parts) = content.as_array() {
        parts
            .iter()
            .filter_map(|p| {
                if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                    p.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        return None;
    };
    let t = raw
        .trim()
        .strip_prefix("<user_query>")
        .unwrap_or(raw.trim());
    let t = t.split("</user_query>").next().unwrap_or(t).trim();
    if t.is_empty() {
        None
    } else {
        Some(t.replace('\n', " "))
    }
}

fn placeholder(title: &str) -> bool {
    title.is_empty() || title.starts_with("New session")
}

fn grok_home() -> PathBuf {
    if let Ok(p) = std::env::var("GROK_HOME") {
        return PathBuf::from(p);
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".grok"))
        .unwrap_or_else(|_| PathBuf::from(".grok"))
}

fn session_dir(cwd: &str) -> PathBuf {
    grok_home().join("sessions").join(encode_cwd(cwd))
}

fn encode_cwd(cwd: &str) -> String {
    let mut out = String::new();
    for b in cwd.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn cmdline(pid: u32) -> String {
    fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| String::from_utf8_lossy(&b).replace('\0', " "))
        .unwrap_or_default()
}

fn terse(s: &str, max: usize) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        return s;
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_encodes_like_grok() {
        assert_eq!(
            encode_cwd("/home/dt/src/witt3rd/smith"),
            "%2Fhome%2Fdt%2Fsrc%2Fwitt3rd%2Fsmith"
        );
    }

    #[test]
    fn user_text_strips_the_query_tag() {
        let v = serde_json::json!({
            "type": "user",
            "content": [{"type": "text", "text": "<user_query>\ncreate smith\n</user_query>"}]
        });
        assert_eq!(user_text(&v).as_deref(), Some("create smith"));
    }

    #[test]
    fn ancestor_walk_stops_at_init() {
        assert!(ancestor_of(std::process::id() as u32, std::process::id() as u32));
        assert!(!ancestor_of(1, 999_999));
    }

    #[test]
    fn summary_prefers_generated_title() {
        let s: Summary = serde_json::from_str(
            r#"{"generated_title":"Create Rust project smith like anvil","session_summary":"older"}"#,
        )
        .unwrap();
        assert_eq!(s.title(), Some("Create Rust project smith like anvil"));
    }
}
