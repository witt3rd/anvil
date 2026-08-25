//! Directories used to launch agents. TUI agents key their inner
//! sessions on cwd. The picker shows this pane's dir, learned roots,
//! and recent launches.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "cwds.json";
const RECENT_CAP: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Places {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Here,
    Root,
    Recent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub kind: Kind,
}

impl Kind {
    pub fn clause(self) -> &'static str {
        match self {
            Kind::Here => "this pane",
            Kind::Root => "root",
            Kind::Recent => "recent",
        }
    }
}

impl Places {
    pub fn load(root: &Path) -> Places {
        let path = root.join(FILE);
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(mut p) = serde_json::from_str::<Places>(&text) {
                p.recent.truncate(RECENT_CAP);
                return p;
            }
        }
        Places::default()
    }

    pub fn save(&self, root: &Path) {
        let _ = std::fs::create_dir_all(root);
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(root.join(FILE), text);
        }
    }

    pub fn remember(&mut self, dir: &str, root: &Path) {
        let dir = normalize(dir);
        if dir.is_empty() {
            return;
        }
        self.recent.retain(|d| d != &dir);
        self.recent.insert(0, dir);
        self.recent.truncate(RECENT_CAP);
        self.save(root);
    }

    /// Parents that cover two or more recent launches. Deeper parents
    /// win when they cover the same set.
    pub fn roots(&self) -> Vec<String> {
        roots_of(&self.recent, &home())
    }

    pub fn rows(&self, here: Option<&str>) -> Vec<Row> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if let Some(here) = here.map(normalize).filter(|s| !s.is_empty()) {
            seen.insert(here.clone());
            out.push(Row {
                path: here,
                kind: Kind::Here,
            });
        }
        for path in self.roots() {
            if seen.insert(path.clone()) {
                out.push(Row {
                    path,
                    kind: Kind::Root,
                });
            }
        }
        for path in &self.recent {
            if seen.insert(path.clone()) {
                out.push(Row {
                    path: path.clone(),
                    kind: Kind::Recent,
                });
            }
        }
        out
    }
}

pub fn home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

pub fn expand(p: &str) -> PathBuf {
    let p = p.trim();
    if p == "~" {
        return home();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return home().join(rest);
    }
    PathBuf::from(p)
}

pub fn normalize(p: &str) -> String {
    let path = expand(p);
    let path = path.canonicalize().unwrap_or(path);
    path.to_string_lossy().trim_end_matches('/').to_string()
}

pub fn is_dir(p: &str) -> bool {
    expand(p).is_dir()
}

pub fn display(p: &str) -> String {
    let h = home();
    let hs = h.to_string_lossy();
    if p == hs.as_ref() {
        return "~".into();
    }
    if let Some(rest) = p.strip_prefix(&format!("{hs}/")) {
        return format!("~/{rest}");
    }
    p.to_string()
}

fn roots_of(recent: &[String], home: &Path) -> Vec<String> {
    let home = home.to_string_lossy().trim_end_matches('/').to_string();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for dir in recent {
        let mut p = Path::new(dir);
        while let Some(parent) = p.parent() {
            let s = parent.to_string_lossy().trim_end_matches('/').to_string();
            if s.is_empty() || s == "/" || s == home {
                break;
            }
            *counts.entry(s.clone()).or_default() += 1;
            p = parent;
        }
    }
    let mut cand: Vec<String> = counts
        .into_iter()
        .filter(|(_, n)| *n >= 2)
        .map(|(p, _)| p)
        .collect();
    cand.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
    let mut out: Vec<String> = Vec::new();
    for path in cand {
        let covered = recent
            .iter()
            .filter(|d| under(d, &path))
            .all(|d| out.iter().any(|root| under(d, root)));
        if covered {
            continue;
        }
        out.push(path);
    }
    out.sort();
    out
}

fn under(dir: &str, root: &str) -> bool {
    dir == root || dir.starts_with(&format!("{root}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_are_the_common_parents() {
        let home = PathBuf::from("/home/dt");
        let recent = vec![
            "/home/dt/src/witt3rd/anvil".into(),
            "/home/dt/src/witt3rd/smith".into(),
            "/home/dt/src/li/being-plugin".into(),
            "/home/dt/src/li/fleet-ops".into(),
        ];
        let roots = roots_of(&recent, &home);
        assert!(roots.contains(&"/home/dt/src/witt3rd".into()), "{roots:?}");
        assert!(roots.contains(&"/home/dt/src/li".into()), "{roots:?}");
        assert!(!roots.contains(&"/home/dt/src".into()), "{roots:?}");
    }

    #[test]
    fn expand_tilde() {
        let h = home();
        assert_eq!(expand("~"), h);
        assert_eq!(expand("~/src"), h.join("src"));
    }
}
