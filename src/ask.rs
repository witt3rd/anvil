//! Ask: model writes Python, anvil strikes, only the result is the answer.
//!
//! `complete` is HTTP chat. This is the agent.

use std::collections::VecDeque;

use thiserror::Error;

use crate::complete::{self, CompleteError, CompleteStats};
use crate::config::Provider;
use crate::frame::{Event, EventBody};
use crate::prof::{self, Timing};
use crate::{Anvil, AnvilError, StrikeReply};

pub const SYSTEM: &str = "\
You are the smith. You write Python for a persistent CPython guest (the hammer).
The harness will exec your code. Print the answer.
You may be shown views of sibling terminals as 'terminal NAME:'.
Rules:
- Reply with Python only. No prose, no markdown, no bash.
- Use pathlib / os / subprocess as needed. The machine is real.
- Print the result. Do not explain it.
";

const MAX_TURNS: usize = 3;
/// Visible events that may enter a model request. Older ones are dropped.
const LOG_WINDOW: usize = 32;

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
    pub timing: Timing,
}

pub trait Completer {
    fn complete(&mut self, messages: &[Message]) -> Result<String, AskError>;
    fn complete_timed(&mut self, messages: &[Message]) -> Result<CompleteStats, AskError> {
        let t0 = std::time::Instant::now();
        let text = self.complete(messages)?;
        Ok(CompleteStats {
            text,
            reasoning: String::new(),
            timing: Timing::wall(t0.elapsed()),
        })
    }
}

pub trait AskSink {
    fn on_status(&mut self, _status: &str) {}
    fn on_draft(&mut self, _text: &str) {}
    fn on_strike(&mut self, _code: &str, _reply: &StrikeReply) {}
    fn on_strike_timed(&mut self, code: &str, reply: &StrikeReply, _timing: &Timing) {
        self.on_strike(code, reply);
    }
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
    ask_with_log(completer, anvil, prompt, &[], sink)
}

/// Like `ask_with`, but the next request is projected from the event log.
/// Only model-visible events enter the prompt.
pub fn ask_with_log(
    completer: &mut impl Completer,
    anvil: &mut Anvil,
    prompt: &str,
    log: &[Event],
    sink: &mut impl AskSink,
) -> Result<AskResult, AskError> {
    let mut messages = messages_from_log(log, prompt);
    let ask_t0 = std::time::Instant::now();
    let mut timing = Timing::default();
    let _ask = prof::span("model.ask", "model");

    for turn in 1..=MAX_TURNS {
        sink.on_status("thinking");
        let stats = completer.complete_timed(&messages)?;
        timing.merge_model(&stats.timing);
        sink.on_draft(&stats.text);
        messages.push(Message {
            role: "assistant".into(),
            content: stats.text.clone(),
        });
        let Some(code) = extract_python(&stats.text) else {
            messages.push(Message {
                role: "user".into(),
                content: "That was not Python. Reply with Python only. Print the answer. No markdown, no bash, no explanation.".into(),
            });
            continue;
        };
        sink.on_status("striking");
        let strike_t0 = std::time::Instant::now();
        let reply = anvil.strike(&code)?;
        let strike_ns = prof::ns(strike_t0.elapsed());
        timing.add_strike(strike_ns);
        let mut strike_t = Timing::wall(strike_t0.elapsed());
        strike_t.strike_ns = Some(strike_ns);
        sink.on_strike_timed(&code, &reply, &strike_t);
        if reply.ok {
            sink.on_status("idle");
            timing.wall_ns = prof::ns(ask_t0.elapsed());
            timing.recompute_tok_s();
            prof::note_model(timing.clone());
            return Ok(AskResult {
                answer: answer_from(&reply),
                code,
                turns: turn,
                reply,
                timing,
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

pub fn messages_from_log(events: &[Event], prompt: &str) -> Vec<Message> {
    let mut messages = vec![Message {
        role: "system".into(),
        content: SYSTEM.into(),
    }];
    let visible: Vec<&EventBody> = events
        .iter()
        .map(|e| &e.body)
        .filter(|b| b.model_visible())
        .collect();
    let start = visible.len().saturating_sub(LOG_WINDOW);
    for body in &visible[start..] {
        match body {
            EventBody::User { text } | EventBody::Ask { prompt: text, .. } => {
                messages.push(Message {
                    role: "user".into(),
                    content: text.clone(),
                });
            }
            EventBody::Strike {
                code, error, ok, ..
            } => {
                messages.push(Message {
                    role: "assistant".into(),
                    content: code.clone(),
                });
                if !ok {
                    messages.push(Message {
                        role: "user".into(),
                        content: format!(
                            "That Python failed:\n{}\nWrite fixed Python only. Print the answer.",
                            error.as_deref().unwrap_or("unknown error")
                        ),
                    });
                }
            }
            EventBody::Answer { text, .. } => {
                if !text.trim().is_empty() {
                    messages.push(Message {
                        role: "user".into(),
                        content: text.clone(),
                    });
                }
            }
            EventBody::See { member, text } => {
                messages.push(Message {
                    role: "user".into(),
                    content: format!("terminal {member}:\n{text}"),
                });
            }
            EventBody::Thinking { .. } | EventBody::Status { .. } | EventBody::Fiber { .. } => {}
        }
    }
    let already = messages
        .last()
        .is_some_and(|m| m.role == "user" && m.content == prompt);
    if !already {
        messages.push(Message {
            role: "user".into(),
            content: prompt.into(),
        });
    }
    messages
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
        Ok(self.complete_timed(messages)?.text)
    }

    fn complete_timed(&mut self, messages: &[Message]) -> Result<CompleteStats, AskError> {
        let pairs: Vec<(&str, &str)> = messages
            .iter()
            .map(|m| (m.role.as_str(), m.content.as_str()))
            .collect();
        Ok(complete::complete_messages_timed(
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
    fn log_projection_skips_invisible_and_does_not_duplicate_prompt() {
        use crate::frame::Event;
        let events = vec![
            Event {
                seq: 0,
                ts: 1,
                body: EventBody::Fiber {
                    state: "hot".into(),
                },
            },
            Event {
                seq: 1,
                ts: 2,
                body: EventBody::Ask {
                    prompt: "what is x".into(),
                    provider: None,
                    model: None,
                    timing: None,
                },
            },
            Event {
                seq: 2,
                ts: 3,
                body: EventBody::Thinking { text: "hmm".into() },
            },
            Event {
                seq: 3,
                ts: 4,
                body: EventBody::Strike {
                    code: "print(x)".into(),
                    stdout: "1\n".into(),
                    stderr: String::new(),
                    error: None,
                    ok: true,
                    ms: Some(2),
                    timing: None,
                },
            },
            Event {
                seq: 4,
                ts: 5,
                body: EventBody::Answer {
                    text: "1".into(),
                    timing: None,
                },
            },
            Event {
                seq: 5,
                ts: 6,
                body: EventBody::Ask {
                    prompt: "double it".into(),
                    provider: None,
                    model: None,
                    timing: None,
                },
            },
        ];
        let msgs = messages_from_log(&events, "double it");
        assert_eq!(msgs[0].role, "system");
        let roles: Vec<_> = msgs.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user", "user"]);
        assert_eq!(msgs[1].content, "what is x");
        assert_eq!(msgs[2].content, "print(x)");
        assert_eq!(msgs[3].content, "1");
        assert_eq!(msgs[4].content, "double it");
        assert!(!msgs.iter().any(|m| m.content.contains("hmm")));
        assert!(!msgs.iter().any(|m| m.content.contains("hot")));
        assert_eq!(msgs.iter().filter(|m| m.content == "double it").count(), 1);
    }

    #[test]
    fn see_is_visible_and_enters_the_prompt() {
        use crate::frame::Event;
        let events = vec![Event {
            seq: 0,
            ts: 1,
            body: EventBody::See {
                member: "bash".into(),
                text: "anvil-pty-ok".into(),
            },
        }];
        assert!(events[0].body.model_visible());
        let msgs = messages_from_log(&events, "what is on the terminal");
        assert!(msgs.iter().any(|m| m.content.contains("terminal bash")));
        assert!(msgs.iter().any(|m| m.content.contains("anvil-pty-ok")));
    }

    #[test]
    fn ask_from_log_passes_prior_to_completer() {
        struct Capture {
            seen: Vec<Vec<String>>,
            next: VecDeque<String>,
        }
        impl Completer for Capture {
            fn complete(&mut self, messages: &[Message]) -> Result<String, AskError> {
                self.seen
                    .push(messages.iter().map(|m| m.content.clone()).collect());
                self.next
                    .pop_front()
                    .ok_or_else(|| AskError::Other("empty".into()))
            }
        }
        let store = tempfile::TempDir::new().unwrap();
        let mut anvil = harness(store.path());
        let events = [crate::frame::Event {
            seq: 0,
            ts: 1,
            body: EventBody::Ask {
                prompt: "prior".into(),
                provider: None,
                model: None,
                timing: None,
            },
        }];
        let mut llm = Capture {
            seen: vec![],
            next: VecDeque::from([String::from("print(2)")]),
        };
        ask_with_log(&mut llm, &mut anvil, "next", &events, &mut ()).unwrap();
        assert!(llm.seen[0].iter().any(|c| c == "prior"));
        assert!(llm.seen[0].iter().any(|c| c == "next"));
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
