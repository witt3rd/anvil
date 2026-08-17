//! Fetch and cache `/models` for a named provider.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{default_cache_dir, Auth, Provider};
use crate::oauth;
use crate::secret;

const FRESH: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("provider has no base_url; cannot query models")]
    NoBaseUrl,
    #[error("provider disables the model catalog (models_path: \"\")")]
    Disabled,
    #[error("no credential for this provider")]
    NoCredential,
    #[error("failed to resolve credential: {0}")]
    Secret(#[from] secret::SecretError),
    #[error("GET {url} failed: {detail}")]
    Http { url: String, detail: String },
    #[error("model list from {url} was not JSON: {detail}")]
    Json { url: String, detail: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Model {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCache {
    pub provider: String,
    pub fetched_at_unix: u64,
    pub url: String,
    pub models: Vec<Model>,
}

impl ModelCache {
    pub fn is_fresh(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.fetched_at_unix) < FRESH.as_secs()
    }
}

pub fn cache_path(provider: &str) -> PathBuf {
    default_cache_dir()
        .join("models")
        .join(format!("{provider}.json"))
}

pub fn load_cache(provider: &str) -> Option<ModelCache> {
    let bytes = std::fs::read(cache_path(provider)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save_cache(cache: &ModelCache) -> Result<PathBuf, CatalogError> {
    let path = cache_path(&cache.provider);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(cache).unwrap())?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

pub fn models(name: &str, provider: &Provider, refresh: bool) -> Result<ModelCache, CatalogError> {
    if !refresh {
        if let Some(cached) = load_cache(name) {
            if cached.is_fresh() {
                return Ok(cached);
            }
        }
    }
    refresh_models(name, provider)
}

pub fn refresh_models(name: &str, provider: &Provider) -> Result<ModelCache, CatalogError> {
    let url = provider.models_url().ok_or_else(|| {
        if provider.models_path.as_deref() == Some("") {
            CatalogError::Disabled
        } else {
            CatalogError::NoBaseUrl
        }
    })?;
    let token = credential(provider)?;
    let mut req = ureq::get(&url)
        .set("authorization", &format!("Bearer {token}"))
        .set("user-agent", concat!("anvil/", env!("CARGO_PKG_VERSION")));
    for (k, v) in provider.resolve_headers()? {
        req = req.set(&k, &v);
    }
    let body = req.call().map_err(|err| CatalogError::Http {
        url: url.clone(),
        detail: err.to_string(),
    })?;
    let body = body.into_string().map_err(|err| CatalogError::Http {
        url: url.clone(),
        detail: err.to_string(),
    })?;
    let list = parse_models(&body).map_err(|detail| CatalogError::Json {
        url: url.clone(),
        detail,
    })?;
    let cache = ModelCache {
        provider: name.to_string(),
        fetched_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        url,
        models: list,
    };
    save_cache(&cache)?;
    Ok(cache)
}

pub fn credential(provider: &Provider) -> Result<String, CatalogError> {
    match &provider.auth {
        Auth::ApiKey { key } => Ok(secret::resolve(key)?),
        Auth::Oauth { vendor } => oauth::cached_token(*vendor).ok_or(CatalogError::NoCredential),
    }
}

pub fn parse_models(body: &str) -> Result<Vec<Model>, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|err| err.to_string())?;
    let rows = if let Some(arr) = value.get("data").and_then(|v| v.as_array()) {
        arr.clone()
    } else if let Some(arr) = value.as_array() {
        arr.clone()
    } else if let Some(arr) = value.get("models").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        return Err("expected {data:[...]}, {models:[...]}, or an array".into());
    };
    let mut models = Vec::new();
    for row in rows {
        let id = row
            .get("id")
            .or_else(|| row.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if id.is_empty() {
            continue;
        }
        let owned_by = row
            .get("owned_by")
            .or_else(|| row.get("ownedBy"))
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        models.push(Model { id, owned_by });
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_shape() {
        let body = r#"{"data":[{"id":"grok-4.5","owned_by":"xai"},{"id":"grok-3"}]}"#;
        let models = parse_models(body).unwrap();
        assert_eq!(models[0].id, "grok-3");
        assert_eq!(models[1].owned_by.as_deref(), Some("xai"));
    }

    #[test]
    fn parses_bare_array() {
        let models = parse_models(r#"[{"id":"a"},{"name":"b"}]"#).unwrap();
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }
}
