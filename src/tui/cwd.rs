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
    Dir,
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
            Kind::Dir => "dir",
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

    /// Empty draft and a complete directory keep the learned list.
    /// A trailing slash lists only that folder's children. A partial
    /// name lists those children first (prefix, then substring), then
    /// other known folders whose names contain the last component.
    pub fn rows_for(&self, here: Option<&str>, draft: &str) -> Vec<Row> {
        self.rows_for_in(here, draft, &home())
    }

    fn rows_for_in(&self, here: Option<&str>, draft: &str, home: &Path) -> Vec<Row> {
        let q = draft.trim();
        if q.is_empty() {
            return self.rows(here);
        }
        if !q.ends_with('/') {
            if let Some(located) = locate_dir_in(&expand(q), home) {
                let typed = located.to_string_lossy().trim_end_matches('/').to_string();
                let mut rows = self.rows(here);
                if let Some(i) = rows.iter().position(|r| r.path == typed) {
                    if i > 0 {
                        let row = rows.remove(i);
                        rows.insert(0, row);
                    }
                } else {
                    rows.insert(
                        0,
                        Row {
                            path: typed,
                            kind: Kind::Dir,
                        },
                    );
                }
                return rows;
            }
        }
        complete(q, self, here, home)
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

const MATCH_CAP: usize = 48;

fn complete(draft: &str, places: &Places, here: Option<&str>, home: &Path) -> Vec<Row> {
    let (parent, needle) = split_draft(draft, home);
    let n = needle.to_lowercase();
    let known = known_kinds(places, here);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    let mut push_ranked = |mut rows: Vec<(String, Kind, u8)>| {
        rows.sort_by(|a, b| {
            a.2.cmp(&b.2).then_with(|| {
                let na = Path::new(&a.0).file_name().unwrap_or_default();
                let nb = Path::new(&b.0).file_name().unwrap_or_default();
                na.cmp(nb).then_with(|| a.0.cmp(&b.0))
            })
        });
        for (path, kind, _) in rows {
            if seen.insert(path.clone()) {
                out.push(Row { path, kind });
            }
        }
    };

    if let Some(ref parent) = parent {
        let primary: Vec<(String, Kind, u8)> = dir_children(parent, &n)
            .into_iter()
            .filter_map(|p| score_name(&p, &n).map(|s| (p.clone(), kind_of(&known, &p), s)))
            .collect();
        push_ranked(primary);
    }

    if n.is_empty() {
        out.truncate(MATCH_CAP);
        return out;
    }

    let mut secondary = Vec::new();
    for row in places.rows(here) {
        if let Some(s) = score_name(&row.path, &n) {
            secondary.push((row.path, row.kind, s));
        }
    }
    for dir in extra_parents(places, here, home, parent.as_deref()) {
        for p in dir_children(&dir, &n) {
            if let Some(s) = score_name(&p, &n) {
                secondary.push((p.clone(), kind_of(&known, &p), s));
            }
        }
    }
    push_ranked(secondary);
    out.truncate(MATCH_CAP);
    out
}

fn split_draft(draft: &str, home: &Path) -> (Option<PathBuf>, String) {
    let t = draft.trim();
    let expanded = expand(t);
    if t.ends_with('/') {
        return (locate_dir_in(&expanded, home), String::new());
    }
    if !t.contains('/') && !t.starts_with('~') {
        return (None, t.to_string());
    }
    let name = expanded
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = expanded
        .parent()
        .map(Path::to_path_buf)
        .filter(|p| !p.as_os_str().is_empty());
    (parent.and_then(|p| locate_dir_in(&p, home)), name)
}

fn locate_dir_in(p: &Path, home: &Path) -> Option<PathBuf> {
    if p.as_os_str().is_empty() {
        return None;
    }
    if p.is_dir() {
        return Some(PathBuf::from(normalize(&p.to_string_lossy())));
    }
    let alt = if p.is_absolute() {
        p.strip_prefix("/").ok().map(|rest| home.join(rest))
    } else {
        Some(home.join(p))
    };
    let alt = alt.filter(|a| a.is_dir())?;
    Some(PathBuf::from(normalize(&alt.to_string_lossy())))
}

fn dir_children(dir: &Path, needle: &str) -> Vec<String> {
    let hidden = needle.starts_with('.');
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name == "." || name == ".." {
            continue;
        }
        if name.starts_with('.') && !hidden {
            continue;
        }
        let p = ent.path();
        if p.is_dir() {
            out.push(normalize(&p.to_string_lossy()));
        }
    }
    out
}

fn score_name(path: &str, needle: &str) -> Option<u8> {
    let name = Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())?;
    if needle.is_empty() {
        return Some(0);
    }
    if name == needle {
        Some(0)
    } else if name.starts_with(needle) {
        Some(1)
    } else if name.contains(needle) {
        Some(2)
    } else {
        None
    }
}

fn known_kinds(places: &Places, here: Option<&str>) -> std::collections::HashMap<String, Kind> {
    places
        .rows(here)
        .into_iter()
        .map(|r| (r.path, r.kind))
        .collect()
}

fn kind_of(known: &std::collections::HashMap<String, Kind>, path: &str) -> Kind {
    known.get(path).copied().unwrap_or(Kind::Dir)
}

fn extra_parents(
    places: &Places,
    here: Option<&str>,
    home: &Path,
    skip: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut add = |p: PathBuf| {
        if skip.is_some_and(|s| s == p.as_path()) {
            return;
        }
        if !p.is_dir() {
            return;
        }
        if dirs.iter().any(|d| d == &p) {
            return;
        }
        dirs.push(p);
    };
    add(home.join("src"));
    for root in places.roots() {
        add(PathBuf::from(root));
    }
    for recent in &places.recent {
        if let Some(p) = Path::new(recent).parent() {
            add(p.to_path_buf());
        }
    }
    if let Some(h) = here {
        let hp = PathBuf::from(h);
        add(hp.clone());
        if let Some(p) = hp.parent() {
            add(p.to_path_buf());
        }
    }
    dirs
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

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| {
                Path::new(&r.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn complete_ranks_parent_prefix_then_elsewhere() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        for p in [
            "src/witt3rd",
            "src/workflow",
            "src/li",
            "src/new-work",
            "other/weird",
            "other/noway",
        ] {
            std::fs::create_dir_all(home.join(p)).unwrap();
        }
        let places = Places {
            recent: vec![
                home.join("src/li").to_string_lossy().into_owned(),
                home.join("other/noway").to_string_lossy().into_owned(),
            ],
        };
        let draft = format!("{}/src/w", home.display());
        let rows = places.rows_for_in(None, &draft, home);
        let names = names(&rows);
        let pos = |n: &str| names.iter().position(|x| x == n);
        assert!(
            pos("witt3rd").unwrap() < pos("new-work").unwrap(),
            "{names:?}"
        );
        assert!(
            pos("workflow").unwrap() < pos("new-work").unwrap(),
            "{names:?}"
        );
        assert!(
            pos("new-work").unwrap() < pos("weird").unwrap(),
            "{names:?}"
        );
        assert!(pos("weird").unwrap() < pos("noway").unwrap(), "{names:?}");
        assert!(!names.contains(&"li".into()), "{names:?}");
    }

    #[test]
    fn trailing_slash_lists_only_that_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        for p in ["src/witt3rd", "src/li", "other/weird", "other/noway"] {
            std::fs::create_dir_all(home.join(p)).unwrap();
        }
        let places = Places {
            recent: vec![
                home.join("src/li").to_string_lossy().into_owned(),
                home.join("other/noway").to_string_lossy().into_owned(),
            ],
        };
        let draft = format!("{}/src/", home.display());
        let listed = names(&places.rows_for_in(None, &draft, home));
        assert!(listed.contains(&"witt3rd".into()), "{listed:?}");
        assert!(listed.contains(&"li".into()), "{listed:?}");
        assert!(!listed.contains(&"weird".into()), "{listed:?}");
        assert!(!listed.contains(&"noway".into()), "{listed:?}");
        let home_rel = names(&places.rows_for_in(None, "/src/", home));
        assert!(home_rel.contains(&"witt3rd".into()), "{home_rel:?}");
        assert!(!home_rel.contains(&"weird".into()), "{home_rel:?}");
    }

    #[test]
    fn complete_slash_src_is_home_src() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        std::fs::create_dir_all(home.join("src/witt3rd")).unwrap();
        std::fs::create_dir_all(home.join("src/li")).unwrap();
        let places = Places::default();
        let rows = places.rows_for_in(None, "/src/w", home);
        let names = names(&rows);
        assert_eq!(
            names.first().map(String::as_str),
            Some("witt3rd"),
            "{names:?}"
        );
    }

    #[test]
    fn complete_dir_keeps_learned_list() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let here = home.join("src/witt3rd");
        std::fs::create_dir_all(&here).unwrap();
        std::fs::create_dir_all(home.join("src/li")).unwrap();
        let here_s = here.to_string_lossy().into_owned();
        let li = home.join("src/li").to_string_lossy().into_owned();
        let places = Places {
            recent: vec![here_s.clone(), li.clone()],
        };
        let rows = places.rows_for_in(Some(&here_s), &here_s, home);
        assert_eq!(rows[0].path, here_s);
        assert_eq!(rows[0].kind, Kind::Here);
        assert!(rows.iter().any(|r| r.path == li));
    }
}
