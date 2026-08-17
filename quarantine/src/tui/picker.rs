//! `@path` fuzzy file picker. Type `@` then a needle; Tab/Enter inserts.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHit {
    pub path: String,
    pub score: i64,
}

/// `@` token just before `cursor`, if it is a file mention (start of input
/// or after whitespace). Query is the text after `@` and contains no spaces.
pub fn at_span(input: &str, cursor: usize) -> Option<(usize, String)> {
    let cursor = cursor.min(input.len());
    if !input.is_char_boundary(cursor) {
        return None;
    }
    let before = &input[..cursor];
    let start = before.rfind('@')?;
    if start > 0 {
        let prev = before[..start].chars().next_back()?;
        if !prev.is_whitespace() && prev != '"' && prev != '\'' && prev != '`' {
            return None;
        }
    }
    let query = &before[start + 1..];
    if query.chars().any(char::is_whitespace) {
        return None;
    }
    Some((start, query.to_string()))
}

pub fn insert_path(input: &str, cursor: usize, path: &str) -> Option<(String, usize)> {
    let (start, _) = at_span(input, cursor)?;
    let mut out = String::with_capacity(input.len() + path.len());
    out.push_str(&input[..start]);
    out.push_str(path);
    out.push(' ');
    let new_cursor = out.len();
    out.push_str(&input[cursor..]);
    Some((out, new_cursor))
}

pub fn rank(files: &[String], needle: &str, limit: usize) -> Vec<FileHit> {
    if needle.is_empty() {
        return files
            .iter()
            .take(limit)
            .map(|path| FileHit {
                path: path.clone(),
                score: 0,
            })
            .collect();
    }
    let needle_l = needle.to_ascii_lowercase();
    let mut hits: Vec<FileHit> = files
        .iter()
        .filter_map(|path| {
            score(&path.to_ascii_lowercase(), &needle_l).map(|score| FileHit {
                path: path.clone(),
                score,
            })
        })
        .collect();
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    hits.truncate(limit);
    hits
}

/// Compact fuzzy: subsequence with bonuses for streak, word start, and
/// matching the file name (not just a parent dir).
fn score(hay: &str, needle: &str) -> Option<i64> {
    if needle.is_empty() {
        return Some(0);
    }
    let h: Vec<char> = hay.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    let mut hi = 0;
    let mut score = 0i64;
    let mut streak = 0i64;
    for &nc in &n {
        let mut found = None;
        for (off, &hc) in h[hi..].iter().enumerate() {
            if hc == nc {
                found = Some(hi + off);
                break;
            }
        }
        let at = found?;
        let mut bonus = 1;
        if at == 0 || matches!(h[at - 1], '/' | '_' | '-' | '.') {
            bonus += 8;
        }
        if at > 0 && h[at - 1] == nc {
            streak += 1;
            bonus += 4 + streak;
        } else {
            streak = 0;
        }
        if let Some(slash) = hay.rfind('/') {
            if at >= slash {
                bonus += 6;
            }
        }
        score += bonus;
        hi = at + 1;
    }
    Some(score - (h.len() as i64 / 8))
}

pub fn list_files(root: &Path, cap: usize) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>, cap: usize) {
        if out.len() >= cap {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut ents: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        ents.sort_by_key(|e| e.file_name());
        for ent in ents {
            if out.len() >= cap {
                return;
            }
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                && matches!(
                    name.as_ref(),
                    ".git" | ".anvil" | ".venv" | ".tox" | ".mypy_cache"
                )
            {
                continue;
            }
            if matches!(
                name.as_ref(),
                "target" | "node_modules" | "__pycache__" | "dist" | "build"
            ) {
                continue;
            }
            let path = ent.path();
            if path.is_dir() {
                walk(&path, root, out, cap);
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    walk(root, root, &mut out, cap);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_span_requires_a_boundary() {
        assert_eq!(at_span("see @src/lib", 12), Some((4, "src/lib".into())));
        assert_eq!(at_span("@", 1), Some((0, String::new())));
        assert_eq!(at_span("mail@host", 9), None);
        assert_eq!(at_span("see @src/foo bar", 16), None);
    }

    #[test]
    fn insert_replaces_the_at_token() {
        let (out, cur) = insert_path("see @li", 7, "src/lib.rs").unwrap();
        assert_eq!(out, "see src/lib.rs ");
        assert_eq!(cur, out.len());
    }

    #[test]
    fn list_files_skips_git_and_is_relative() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".git/HEAD"), b"ref").unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/lib.rs"), b"x").unwrap();
        let files = list_files(tmp.path(), 100);
        assert!(files.iter().any(|f| f == "src/lib.rs"));
        assert!(files.iter().all(|f| !f.contains(".git")));
    }

    #[test]
    fn fuzzy_prefers_filename_over_parent() {
        let files = [
            "vendor/lib.rs".into(),
            "src/lib.rs".into(),
            "src/bin/smith.rs".into(),
        ];
        let hits = rank(&files, "lib", 3);
        assert_eq!(hits[0].path, "src/lib.rs");
        let smith = rank(&files, "smi", 3);
        assert_eq!(smith[0].path, "src/bin/smith.rs");
    }
}
