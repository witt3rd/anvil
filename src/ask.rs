//! Ask: model writes Python, anvil strikes, only the result is the answer.
//!
//! `complete` is HTTP chat. This is the agent.

use std::collections::VecDeque;

use thiserror::Error;

use crate::complete::{self, CompleteError};
use crate::config::Provider;
use crate::{Anvil, AnvilError, StrikeReply};

pub const SYSTEM: &str = "\
You are the smith. You write Python for a persistent CPython guest (the hammer).
The harness will exec your code. Print the answer.
Rules:
- Reply with Python only. No prose, no markdown, no bash.
- Use pathlib / os / subprocess as needed. The machine is real.
- Print the result. Do not explain it.
";

const MAX_TURNS: usize = 3;

#[derive(Debug, Error)]
pub enum AskError {
    #[error("completer returned no Python after {0} turns")]
    NoCode(usize),
    #[error("strike failed: {0}")]
    Strike(#[from] AnvilError),
    #[error(transparent)]
    Complete(#[from] CompleteError),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct AskResult {
    pub answer: String,
    pub code: String,
    pub turns: usize,
    pub reply: StrikeReply,
}

pub trait Completer {
    fn complete(&mut self, messages: &[Message]) -> Result<String, AskError>;
}

pub trait AskSink {
    fn on_status(&mut self, _status: &str) {}
    fn on_draft(&mut self, _text: &str) {}
    fn on_strike(&mut self, _code: &str, _reply: &StrikeReply) {}
}

impl AskSink for () {}

pub fn ask(
    completer: &mut impl Completer,
    anvil: &mut Anvil,
    prompt: &str,
) -> Result<AskResult, AskError> {
    ask_with(completer, anvil, prompt, &mut ())
}

pub fn ask_with(
    completer: &mut impl Completer,
    anvil: &mut Anvil,
    prompt: &str,
    sink: &mut impl AskSink,
) -> Result<AskResult, AskError> {
    let mut messages = vec![
        Message {
            role: "system".into(),
            content: SYSTEM.into(),
        },
        Message {
            role: "user".into(),
            content: prompt.into(),
        },
    ];

    for turn in 1..=MAX_TURNS {
        sink.on_status("thinking");
        let draft = completer.complete(&messages)?;
        sink.on_draft(&draft);
        messages.push(Message {
            role: "assistant".into(),
            content: draft.clone(),
        });
        let Some(code) = extract_python(&draft) else {
            messages.push(Message {
                role: "user".into(),
                content: "That was not Python. Reply with Python only. Print the answer. No markdown, no bash, no explanation.".into(),
            });
            continue;
        };
        sink.on_status("striking");
        let reply = anvil.strike(&code)?;
        sink.on_strike(&code, &reply);
        if reply.ok {
            sink.on_status("idle");
            return Ok(AskResult {
                answer: answer_from(&reply),
                code,
                turns: turn,
                reply,
            });
        }
        messages.push(Message {
            role: "user".into(),
            content: format!(
                "That Python failed:\n{}\nWrite fixed Python only. Print the answer.",
                reply.error.as_deref().unwrap_or("unknown error")
            ),
        });
    }
    sink.on_status("idle");
    Err(AskError::NoCode(MAX_TURNS))
}

pub fn extract_python(draft: &str) -> Option<String> {
    let trimmed = draft.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(code) = fenced(trimmed, &["python", "py"]) {
        return Some(code);
    }
    if fenced(trimmed, &["bash", "sh", "zsh", "shell"]).is_some() && !looks_like_python(trimmed) {
        return None;
    }
    if let Some(code) = fenced(trimmed, &[""]) {
        if looks_like_python(&code) {
            return Some(code);
        }
        return None;
    }
    if looks_like_python(trimmed) && !looks_like_prose(trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

fn fenced(text: &str, langs: &[&str]) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(rel) = text[i..].find("```") {
        let start = i + rel + 3;
        let rest = &text[start..];
        let (lang, after_lang) = match rest.find('\n') {
            Some(n) => (rest[..n].trim(), &rest[n + 1..]),
            None => break,
        };
        let lang_ok = langs.iter().any(|want| {
            if want.is_empty() {
                lang.is_empty()
            } else {
                lang.eq_ignore_ascii_case(want)
            }
        });
        if let Some(end) = after_lang.find("```") {
            if lang_ok {
                return Some(after_lang[..end].trim().to_string());
            }
            i = start + (rest.len() - after_lang.len()) + end + 3;
            continue;
        }
        let _ = bytes;
        break;
    }
    None
}

fn looks_like_python(text: &str) -> bool {
    let body = text.trim();
    if body.is_empty() {
        return false;
    }
    let first = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let t = first.trim_start();
    t.starts_with("import ")
        || t.starts_with("from ")
        || t.starts_with("print(")
        || t.starts_with("def ")
        || t.starts_with("class ")
        || t.starts_with("async ")
        || t.starts_with("for ")
        || t.starts_with("if ")
        || t.starts_with("with ")
        || t.starts_with("try:")
        || t.contains("Path(")
}

fn looks_like_prose(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("here's")
        || lower.contains("here is")
        || lower.contains("### ")
        || lower.contains("one-liner")
        || lower.contains("what this does")
}

fn answer_from(reply: &StrikeReply) -> String {
    let out = reply.stdout.trim();
    if !out.is_empty() {
        return out.to_string();
    }
    if reply.value.is_null() {
        String::new()
    } else if let Some(s) = reply.value.as_str() {
        s.to_string()
    } else {
        reply.value.to_string()
    }
}

pub struct ScriptedCompleter {
    next: VecDeque<String>,
}

impl ScriptedCompleter {
    pub fn new(replies: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            next: replies.into_iter().map(Into::into).collect(),
        }
    }
}

impl Completer for ScriptedCompleter {
    fn complete(&mut self, _messages: &[Message]) -> Result<String, AskError> {
        self.next
            .pop_front()
            .ok_or_else(|| AskError::Other("scripted completer exhausted".into()))
    }
}

pub struct HttpCompleter {
    pub provider: Provider,
    pub model: String,
}

impl Completer for HttpCompleter {
    fn complete(&mut self, messages: &[Message]) -> Result<String, AskError> {
        let pairs: Vec<(&str, &str)> = messages
            .iter()
            .map(|m| (m.role.as_str(), m.content.as_str()))
            .collect();
        Ok(complete::complete_messages(
            &self.provider,
            &self.model,
            &pairs,
        )?)
    }
}

/// The waffle `anvil complete` actually returned for the symlink prompt.
pub const WAFFLE: &str = r#"The interpretation of "have synlinks" usually means: **count all symbolic link files** found recursively inside `~/dotfiles/`.

Here's the standard one-liner to get that count:

```bash
find ~/dotfiles -type l | wc -l
```

### What this does
- `find ~/dotfiles` — searches recursively
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_hammer, Anvil};
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn tree() -> (TempDir, usize) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        std::fs::write(root.join("b.txt"), b"b").unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/c.txt"), b"c").unwrap();
        symlink(root.join("a.txt"), root.join("link1")).unwrap();
        symlink(root.join("b.txt"), root.join("sub/link2")).unwrap();
        symlink(root.join("missing"), root.join("broken")).unwrap();
        (tmp, 3)
    }

    fn harness(store: &std::path::Path) -> Anvil {
        Anvil::open(store, default_hammer()).unwrap()
    }

    fn count_code(root: &std::path::Path) -> String {
        format!(
            "from pathlib import Path\nprint(sum(1 for p in Path(r\"{}\").rglob(\"*\") if p.is_symlink()))\n",
            root.display()
        )
    }

    #[test]
    fn extract_skips_bash_waffle() {
        assert!(extract_python(WAFFLE).is_none());
    }

    #[test]
    fn extract_takes_python_fence() {
        let draft = "sure\n```python\nprint(1)\n```\n";
        assert_eq!(extract_python(draft).as_deref(), Some("print(1)"));
    }

    #[test]
    fn ask_rejects_waffle_and_strikes_python() {
        let (tmp, expected) = tree();
        let store = tempfile::TempDir::new().unwrap();
        let mut anvil = harness(store.path());
        let prompt = format!(
            "how many files have synlinks {} (recursive)",
            tmp.path().display()
        );
        let mut llm = ScriptedCompleter::new([WAFFLE.to_string(), count_code(tmp.path())]);
        let result = ask(&mut llm, &mut anvil, &prompt).unwrap();
        assert_eq!(result.answer.trim(), expected.to_string());
        assert_eq!(result.turns, 2);
        assert!(!result.answer.to_ascii_lowercase().contains("one-liner"));
        assert!(!result.answer.contains("find "));
    }

    #[test]
    fn sink_sees_waffle_then_strike() {
        let (tmp, expected) = tree();
        let store = tempfile::TempDir::new().unwrap();
        let mut anvil = harness(store.path());
        let prompt = format!(
            "how many files have synlinks {} (recursive)",
            tmp.path().display()
        );
        let mut llm = ScriptedCompleter::new([WAFFLE.to_string(), count_code(tmp.path())]);
        let mut rec = Rec::default();
        let result = ask_with(&mut llm, &mut anvil, &prompt, &mut rec).unwrap();
        assert_eq!(result.answer.trim(), expected.to_string());
        assert_eq!(rec.drafts.len(), 2);
        assert_eq!(rec.strikes.len(), 1);
        assert!(rec.drafts[0].contains("one-liner"));
    }

    #[derive(Default)]
    struct Rec {
        drafts: Vec<String>,
        strikes: Vec<String>,
    }

    impl AskSink for Rec {
        fn on_draft(&mut self, text: &str) {
            self.drafts.push(text.into());
        }
        fn on_strike(&mut self, code: &str, _reply: &StrikeReply) {
            self.strikes.push(code.into());
        }
    }

    #[test]
    fn ask_fails_if_the_model_only_waffles() {
        let (tmp, _) = tree();
        let store = tempfile::TempDir::new().unwrap();
        let mut anvil = harness(store.path());
        let prompt = format!(
            "how many files have synlinks {} (recursive)",
            tmp.path().display()
        );
        let mut llm = ScriptedCompleter::new([WAFFLE, WAFFLE, WAFFLE]);
        let err = ask(&mut llm, &mut anvil, &prompt).unwrap_err();
        assert!(matches!(err, AskError::NoCode(3)));
    }
}
