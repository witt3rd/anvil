//! OpenAI-compatible chat. Stream when we can so prefill / TTFT / decode
//! / reasoning / tok/s are measured, not guessed.

use std::io::{BufRead, BufReader};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::{self, CatalogError};
use crate::config::Provider;
use crate::prof::{self, Timing};

#[derive(Debug, Error)]
pub enum CompleteError {
    #[error("provider has no base_url")]
    NoBaseUrl,
    #[error("no model: set default_model or pass --model")]
    NoModel,
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Secret(#[from] crate::secret::SecretError),
    #[error("POST {url} failed: {detail}")]
    Http { url: String, detail: String },
}

#[derive(Debug, Clone)]
pub struct CompleteStats {
    pub text: String,
    pub reasoning: String,
    pub timing: Timing,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Option<Vec<Choice>>,
    usage: Option<Usage>,
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Option<ChoiceMessage>,
    delta: Option<Delta>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: Option<String>,
}

pub fn complete(provider: &Provider, model: &str, prompt: &str) -> Result<String, CompleteError> {
    Ok(complete_messages_timed(provider, model, &[("user", prompt)])?.text)
}

pub fn complete_messages(
    provider: &Provider,
    model: &str,
    messages: &[(&str, &str)],
) -> Result<String, CompleteError> {
    Ok(complete_messages_timed(provider, model, messages)?.text)
}

pub fn complete_messages_timed(
    provider: &Provider,
    model: &str,
    messages: &[(&str, &str)],
) -> Result<CompleteStats, CompleteError> {
    complete_messages_timed_with(provider, model, messages, |_, _| {})
}

pub fn complete_messages_timed_with(
    provider: &Provider,
    model: &str,
    messages: &[(&str, &str)],
    on_delta: impl FnMut(&str, &str),
) -> Result<CompleteStats, CompleteError> {
    let model = model.trim();
    if model.is_empty() {
        return Err(CompleteError::NoModel);
    }
    let _span = prof::span("model.complete", "model");
    let base = provider.base_url().ok_or(CompleteError::NoBaseUrl)?;
    let url = format!("{base}/chat/completions");
    let token = catalog::credential(provider)?;
    let body = ChatRequest {
        model,
        messages: messages
            .iter()
            .map(|(role, content)| ChatMessage { role, content })
            .collect(),
        stream: true,
        stream_options: Some(StreamOptions {
            include_usage: true,
        }),
    };
    let mut req = ureq::post(&url)
        .set("authorization", &format!("Bearer {token}"))
        .set("content-type", "application/json")
        .set("user-agent", concat!("anvil/", env!("CARGO_PKG_VERSION")));
    for (k, v) in provider.resolve_headers()? {
        req = req.set(&k, &v);
    }
    let t0 = Instant::now();
    let resp = req
        .send_json(serde_json::to_value(&body).unwrap())
        .map_err(|err| CompleteError::Http {
            url: url.clone(),
            detail: err.to_string(),
        })?;
    let reader = BufReader::new(resp.into_reader());
    let stats = read_completion(&url, reader, t0, on_delta)?;
    prof::counter(
        "model.tokens_out",
        stats.timing.tokens_out.unwrap_or(0) as u64,
    );
    if let Some(ns) = stats.timing.prefill_ns.or(stats.timing.ttft_ns) {
        prof::record(prof::Sample {
            name: "model.prefill".into(),
            group: "model".into(),
            t0_ns: prof::now_ns().saturating_sub(stats.timing.wall_ns),
            dur_ns: ns,
            tokens: stats.timing.tokens_in,
            extra: None,
        });
    }
    if let Some(ns) = stats.timing.decode_ns {
        prof::record(prof::Sample {
            name: "model.decode".into(),
            group: "model".into(),
            t0_ns: prof::now_ns().saturating_sub(ns),
            dur_ns: ns,
            tokens: stats.timing.tokens_out,
            extra: stats.timing.tok_s.map(|s| format!("{s:.1} tok/s")),
        });
    }
    if let Some(ns) = stats.timing.reason_ns {
        prof::record(prof::Sample {
            name: "model.reason".into(),
            group: "model".into(),
            t0_ns: prof::now_ns().saturating_sub(ns),
            dur_ns: ns,
            tokens: None,
            extra: None,
        });
    }
    prof::note_model(stats.timing.clone());
    Ok(stats)
}

fn read_completion(
    url: &str,
    reader: impl BufRead,
    t0: Instant,
    mut on_delta: impl FnMut(&str, &str),
) -> Result<CompleteStats, CompleteError> {
    let mut lines = reader.lines();
    let first = loop {
        match lines.next() {
            None => {
                return Err(CompleteError::Http {
                    url: url.into(),
                    detail: "empty completion".into(),
                })
            }
            Some(Err(err)) => {
                return Err(CompleteError::Http {
                    url: url.into(),
                    detail: err.to_string(),
                })
            }
            Some(Ok(line)) if line.trim().is_empty() => continue,
            Some(Ok(line)) => break line,
        }
    };
    if first.trim_start().starts_with('{') {
        let mut rest = first;
        rest.push('\n');
        for line in lines {
            rest.push_str(&line.map_err(|err| CompleteError::Http {
                url: url.into(),
                detail: err.to_string(),
            })?);
            rest.push('\n');
        }
        return from_json(url, &rest, t0);
    }
    let mut acc = StreamAcc::new(t0);
    acc.ingest_line(&first, &mut on_delta);
    for line in lines {
        let line = line.map_err(|err| CompleteError::Http {
            url: url.into(),
            detail: err.to_string(),
        })?;
        acc.ingest_line(&line, &mut on_delta);
    }
    acc.finish(url)
}

struct StreamAcc {
    t0: Instant,
    first_any: Option<Instant>,
    first_content: Option<Instant>,
    first_reason: Option<Instant>,
    last_reason: Option<Instant>,
    last_token: Option<Instant>,
    text: String,
    reasoning: String,
    tokens_in: Option<u32>,
    tokens_out: Option<u32>,
    error: Option<String>,
    in_think: bool,
}

impl StreamAcc {
    fn new(t0: Instant) -> Self {
        Self {
            t0,
            first_any: None,
            first_content: None,
            first_reason: None,
            last_reason: None,
            last_token: None,
            text: String::new(),
            reasoning: String::new(),
            tokens_in: None,
            tokens_out: None,
            error: None,
            in_think: false,
        }
    }

    fn mark_any(&mut self) {
        let now = Instant::now();
        if self.first_any.is_none() {
            self.first_any = Some(now);
        }
    }

    fn ingest_line(&mut self, line: &str, on_delta: &mut impl FnMut(&str, &str)) {
        if let Some(delta) = parse_sse_line(line) {
            if let Some(err) = delta.error {
                self.error = Some(err);
                return;
            }
            if let Some((pin, cout)) = delta.usage {
                self.tokens_in = pin.or(self.tokens_in);
                self.tokens_out = cout.or(self.tokens_out);
            }
            if let Some(r) = delta.reasoning {
                if !r.is_empty() {
                    self.mark_any();
                    let now = Instant::now();
                    if self.first_reason.is_none() {
                        self.first_reason = Some(now);
                    }
                    self.last_reason = Some(now);
                    self.reasoning.push_str(&r);
                    on_delta("reason", &r);
                }
            }
            if let Some(c) = delta.content {
                if !c.is_empty() {
                    self.push_content(&c, on_delta);
                }
            }
        }
    }

    fn push_content(&mut self, chunk: &str, on_delta: &mut impl FnMut(&str, &str)) {
        self.mark_any();
        let now = Instant::now();
        if chunk.contains("<think>") {
            self.in_think = true;
            if self.first_reason.is_none() {
                self.first_reason = Some(now);
            }
        }
        if self.in_think {
            self.last_reason = Some(now);
            self.reasoning.push_str(chunk);
            on_delta("reason", chunk);
            if chunk.contains("</think>") {
                self.in_think = false;
            }
            return;
        }
        if self.first_content.is_none() {
            self.first_content = Some(now);
        }
        self.last_token = Some(now);
        self.text.push_str(chunk);
        on_delta("content", chunk);
    }

    fn finish(self, url: &str) -> Result<CompleteStats, CompleteError> {
        if let Some(err) = self.error {
            return Err(CompleteError::Http {
                url: url.into(),
                detail: err,
            });
        }
        let text = self.text;
        if text.is_empty() && self.reasoning.is_empty() {
            return Err(CompleteError::Http {
                url: url.into(),
                detail: "empty completion".into(),
            });
        }
        let wall = self.t0.elapsed();
        let first = self.first_any.or(self.first_content);
        let prefill = first.map(|t| t.saturating_duration_since(self.t0));
        let ttft = self
            .first_content
            .or(self.first_any)
            .map(|t| t.saturating_duration_since(self.t0));
        let decode = match (
            self.first_content.or(self.first_any),
            self.last_token.or(self.last_reason),
        ) {
            (Some(a), Some(b)) if b > a => Some(b.saturating_duration_since(a)),
            _ => None,
        };
        let reason = match (self.first_reason, self.last_reason) {
            (Some(a), Some(b)) if b >= a => Some(b.saturating_duration_since(a)),
            _ => None,
        };
        let tokens_out = self.tokens_out.or_else(|| {
            let n = prof::estimate_tokens(&text);
            if n == 0 {
                None
            } else {
                Some(n)
            }
        });
        let mut timing = Timing {
            wall_ns: prof::ns(wall),
            prefill_ns: prefill.map(prof::ns),
            ttft_ns: ttft.map(prof::ns),
            decode_ns: decode.map(prof::ns),
            reason_ns: reason.map(prof::ns),
            strike_ns: None,
            tokens_in: self.tokens_in,
            tokens_out,
            tok_s: None,
        };
        timing.recompute_tok_s();
        Ok(CompleteStats {
            text,
            reasoning: strip_think(&self.reasoning),
            timing,
        })
    }
}

fn from_json(url: &str, text: &str, t0: Instant) -> Result<CompleteStats, CompleteError> {
    let parsed: ChatResponse = serde_json::from_str(text).map_err(|err| CompleteError::Http {
        url: url.into(),
        detail: err.to_string(),
    })?;
    if let Some(err) = parsed.error {
        return Err(CompleteError::Http {
            url: url.into(),
            detail: err.message.unwrap_or_else(|| text.to_string()),
        });
    }
    let choice = parsed
        .choices
        .into_iter()
        .flatten()
        .find_map(|c| c.message)
        .ok_or_else(|| CompleteError::Http {
            url: url.into(),
            detail: "empty completion".into(),
        })?;
    let text = choice.content.unwrap_or_default();
    let reasoning = choice
        .reasoning_content
        .or(choice.reasoning)
        .unwrap_or_default();
    if text.is_empty() && reasoning.is_empty() {
        return Err(CompleteError::Http {
            url: url.into(),
            detail: "empty completion".into(),
        });
    }
    let mut timing = Timing::wall(t0.elapsed());
    timing.ttft_ns = Some(timing.wall_ns);
    timing.prefill_ns = Some(timing.wall_ns);
    timing.tokens_in = parsed.usage.as_ref().and_then(|u| u.prompt_tokens);
    timing.tokens_out = parsed
        .usage
        .as_ref()
        .and_then(|u| u.completion_tokens)
        .or_else(|| Some(prof::estimate_tokens(&text)));
    Ok(CompleteStats {
        text,
        reasoning,
        timing,
    })
}

#[derive(Debug, Default)]
pub(crate) struct SseDelta {
    content: Option<String>,
    reasoning: Option<String>,
    usage: Option<(Option<u32>, Option<u32>)>,
    error: Option<String>,
}

pub(crate) fn parse_sse_line(line: &str) -> Option<SseDelta> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let payload = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
    if payload == "[DONE]" {
        return None;
    }
    if !payload.starts_with('{') {
        return None;
    }
    let parsed: ChatResponse = serde_json::from_str(payload).ok()?;
    if let Some(err) = parsed.error {
        return Some(SseDelta {
            error: Some(err.message.unwrap_or_else(|| payload.to_string())),
            ..SseDelta::default()
        });
    }
    let mut out = SseDelta::default();
    if let Some(u) = parsed.usage {
        out.usage = Some((u.prompt_tokens, u.completion_tokens));
    }
    if let Some(d) = parsed.choices.into_iter().flatten().find_map(|c| c.delta) {
        out.content = d.content.filter(|s| !s.is_empty());
        out.reasoning = d
            .reasoning_content
            .or(d.reasoning)
            .filter(|s| !s.is_empty());
    }
    Some(out)
}

fn strip_think(s: &str) -> String {
    s.replace("<think>", "").replace("</think>", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_content_and_reasoning_and_usage() {
        let c = parse_sse_line(r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#).unwrap();
        assert_eq!(c.content.as_deref(), Some("hi"));
        let r =
            parse_sse_line(r#"data: {"choices":[{"delta":{"reasoning_content":"hmm"}}]}"#).unwrap();
        assert_eq!(r.reasoning.as_deref(), Some("hmm"));
        let u = parse_sse_line(r#"data: {"usage":{"prompt_tokens":10,"completion_tokens":4}}"#)
            .unwrap();
        assert_eq!(u.usage, Some((Some(10), Some(4))));
        assert!(parse_sse_line("data: [DONE]").is_none());
    }

    #[test]
    fn stream_acc_folds_ttft_and_tok_s() {
        let t0 = Instant::now();
        let mut acc = StreamAcc::new(t0);
        let mut saw = Vec::new();
        acc.ingest_line(
            r#"data: {"choices":[{"delta":{"reasoning_content":"plan"}}]}"#,
            &mut |k, t| saw.push((k.to_string(), t.to_string())),
        );
        acc.ingest_line(
            r#"data: {"choices":[{"delta":{"content":"print(1)"}}]}"#,
            &mut |k, t| saw.push((k.to_string(), t.to_string())),
        );
        acc.ingest_line(
            r#"data: {"usage":{"prompt_tokens":8,"completion_tokens":2}}"#,
            &mut |_, _| {},
        );
        assert_eq!(saw[0], ("reason".into(), "plan".into()));
        assert_eq!(saw[1], ("content".into(), "print(1)".into()));
        let stats = acc.finish("http://x").unwrap();
        assert_eq!(stats.text, "print(1)");
        assert!(stats.reasoning.contains("plan"));
        assert!(stats.timing.ttft_ns.is_some());
        assert_eq!(stats.timing.tokens_out, Some(2));
    }
}
