//! One-shot OpenAI-compatible chat completion. The model loop will sit on this.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::{self, CatalogError};
use crate::config::Provider;

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

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Option<Vec<Choice>>,
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Option<ChoiceMessage>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: Option<String>,
}

pub fn complete(provider: &Provider, model: &str, prompt: &str) -> Result<String, CompleteError> {
    complete_messages(provider, model, &[("user", prompt)])
}

pub fn complete_messages(
    provider: &Provider,
    model: &str,
    messages: &[(&str, &str)],
) -> Result<String, CompleteError> {
    let model = model.trim();
    if model.is_empty() {
        return Err(CompleteError::NoModel);
    }
    let base = provider.base_url().ok_or(CompleteError::NoBaseUrl)?;
    let url = format!("{base}/chat/completions");
    let token = catalog::credential(provider)?;
    let body = ChatRequest {
        model,
        messages: messages
            .iter()
            .map(|(role, content)| ChatMessage { role, content })
            .collect(),
    };
    let mut req = ureq::post(&url)
        .set("authorization", &format!("Bearer {token}"))
        .set("content-type", "application/json")
        .set("user-agent", concat!("anvil/", env!("CARGO_PKG_VERSION")));
    for (k, v) in provider.resolve_headers()? {
        req = req.set(&k, &v);
    }
    let text = req
        .send_json(serde_json::to_value(&body).unwrap())
        .map_err(|err| CompleteError::Http {
            url: url.clone(),
            detail: err.to_string(),
        })?
        .into_string()
        .map_err(|err| CompleteError::Http {
            url: url.clone(),
            detail: err.to_string(),
        })?;
    let parsed: ChatResponse = serde_json::from_str(&text).map_err(|err| CompleteError::Http {
        url: url.clone(),
        detail: err.to_string(),
    })?;
    if let Some(err) = parsed.error {
        return Err(CompleteError::Http {
            url,
            detail: err.message.unwrap_or_else(|| text),
        });
    }
    parsed
        .choices
        .into_iter()
        .flatten()
        .find_map(|c| c.message.and_then(|m| m.content))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CompleteError::Http {
            url,
            detail: "empty completion".into(),
        })
}
