//! Process-tree door: a turn is a descendant cmdline that matches
//! every `contains` needle. Optional session files under `$HOME/<home>`.

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use super::adopt;
use crate::catalog::SessionFiles;

pub fn session_id(pid: u32, files: &SessionFiles) -> Option<String> {
    live(pid, files).map(|h| h.session_id)
}

pub fn activity(pid: u32, files: &SessionFiles) -> Option<String> {
    let hit = live(pid, files)?;
    if let Some(title) = read_title(&hit, files).filter(|t| !placeholder(t)) {
        return Some(terse(&title, 28));
    }
    last_user(&hit, files).map(|s| terse(&s, 28))
}

pub fn turning(pid: u32, needles: &[String]) -> bool {
    if needles.is_empty() {
        return false;
    }
    tree_matches(pid, needles)
}

fn tree_matches(root: u32, needles: &[String]) -> bool {
    let mut pids = vec![root];
    pids.extend(adopt::descendants(root));
    pids.iter().any(|p| {
        let cmd = cmdline(*p);
        needles.iter().all(|n| cmd.contains(n.as_str()))
    })
}

#[derive(Debug, Clone)]
struct Hit {
    session_id: String,
    cwd: String,
}

#[derive(Debug, Deserialize)]
struct Active {
    session_id: String,
    pid: u32,
    cwd: String,
}

fn live(root: u32, files: &SessionFiles) -> Option<Hit> {
    let active: Vec<Active> = serde_json::from_str(
        &fs::read_to_string(product_home(&files.home).join(&files.active)).ok()?,
    )
    .ok()?;
    for a in &active {
        if a.pid == root || ancestor_of(a.pid, root) {
            return Some(Hit {
                session_id: a.session_id.clone(),
                cwd: a.cwd.clone(),
            });
        }
    }
    let down: Vec<u32> = std::iter::once(root)
        .chain(adopt::descendants(root))
        .collect();
    active
        .into_iter()
        .find(|a| down.contains(&a.pid))
        .map(|a| Hit {
            session_id: a.session_id,
            cwd: a.cwd,
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

fn read_title(hit: &Hit, files: &SessionFiles) -> Option<String> {
    let text = fs::read_to_string(
        session_dir(&files.home, &hit.cwd)
            .join(&hit.session_id)
            .join(&files.summary),
    )
    .ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    for key in &files.title_keys {
        if let Some(s) = v
            .get(key)
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
        {
            return Some(s.to_string());
        }
    }
    None
}

fn last_user(hit: &Hit, files: &SessionFiles) -> Option<String> {
    let path = session_dir(&files.home, &hit.cwd)
        .join(&hit.session_id)
        .join(&files.history);
    let text = fs::read_to_string(path).ok()?;
    let mut last = None;
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        if let Some(s) = user_text(&v, &files.strip_tags) {
            last = Some(s);
            break;
        }
    }
    last
}

fn user_text(v: &serde_json::Value, strip_tags: &[String]) -> Option<String> {
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
    let mut t = raw.trim().to_string();
    for tag in strip_tags {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        if let Some(rest) = t.strip_prefix(&open) {
            t = rest.split(&close).next().unwrap_or(rest).trim().to_string();
        }
    }
    if t.is_empty() {
        None
    } else {
        Some(t.replace('\n', " "))
    }
}

fn placeholder(title: &str) -> bool {
    title.is_empty() || title.starts_with("New session")
}

fn product_home(home: &str) -> PathBuf {
    let home = home.trim_start_matches('/');
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(home))
        .unwrap_or_else(|_| PathBuf::from(home))
}

fn session_dir(home: &str, cwd: &str) -> PathBuf {
    product_home(home).join("sessions").join(encode_cwd(cwd))
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
    fn cwd_percent_encodes_paths() {
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
        assert_eq!(
            user_text(&v, &["user_query".into()]).as_deref(),
            Some("create smith")
        );
    }

    #[test]
    fn ancestor_walk_stops_at_init() {
        assert!(ancestor_of(
            std::process::id() as u32,
            std::process::id() as u32
        ));
        assert!(!ancestor_of(1, 999_999));
    }

    #[test]
    fn title_keys_pick_the_first_hit() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"generated_title":"Create Rust project smith like anvil","session_summary":"older"}"#,
        )
        .unwrap();
        let title = ["generated_title", "session_summary"]
            .iter()
            .find_map(|k| v.get(*k).and_then(|x| x.as_str()).filter(|s| !s.is_empty()));
        assert_eq!(title, Some("Create Rust project smith like anvil"));
    }
}
