//! ~/.config/anvil/config.yaml — named providers, nothing first-class.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::secret::{self, SecretError};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config not found at {}", .0.display())]
    Missing(PathBuf),
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid YAML in {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("unknown provider '{0}'")]
    UnknownProvider(String),
    #[error("no providers configured in {}", .0.display())]
    Empty(PathBuf),
    #[error("provider '{0}' has no default and none was requested")]
    NoDefault(String),
    #[error(transparent)]
    Secret(#[from] SecretError),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    #[serde(default)]
    pub providers: BTreeMap<String, Provider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    /// OpenAI-compatible base, e.g. `https://api.x.ai/v1` or `http://127.0.0.1:20128/v1`.
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth: Auth,
    /// Extra headers. Values go through the same secret resolver as keys.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub default_model: Option<String>,
    /// GET path for the model list. Default `/models`. Empty string = no catalog.
    #[serde(default)]
    pub models_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Auth {
    ApiKey { key: String },
    Oauth { vendor: OauthVendor },
}

impl Default for Auth {
    fn default() -> Self {
        Auth::ApiKey { key: String::new() }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OauthVendor {
    Grok,
}

impl Config {
    pub fn load() -> Result<(PathBuf, Self), ConfigError> {
        Self::load_from(default_config_path())
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<(PathBuf, Self), ConfigError> {
        let path = path.as_ref().to_path_buf();
        if !path.is_file() {
            return Err(ConfigError::Missing(path));
        }
        let bytes = std::fs::read(&path).map_err(|source| ConfigError::Io {
            path: path.clone(),
            source,
        })?;
        let cfg: Config = serde_yaml::from_slice(&bytes).map_err(|source| ConfigError::Yaml {
            path: path.clone(),
            source,
        })?;
        if cfg.providers.is_empty() {
            return Err(ConfigError::Empty(path));
        }
        Ok((path, cfg))
    }

    pub fn provider(&self, name: Option<&str>) -> Result<(&str, &Provider), ConfigError> {
        let key = name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or(self.default_provider.as_deref())
            .ok_or_else(|| ConfigError::NoDefault("provider".into()))?;
        let (name, provider) = self
            .providers
            .get_key_value(key)
            .ok_or_else(|| ConfigError::UnknownProvider(key.to_string()))?;
        Ok((name.as_str(), provider))
    }

    pub fn model_for(&self, provider: &Provider, requested: Option<&str>) -> Option<String> {
        requested
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or_else(|| provider.default_model.clone())
            .or_else(|| self.default_model.clone())
    }
}

impl Provider {
    pub fn base_url(&self) -> Option<String> {
        self.base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_end_matches('/').to_string())
    }

    pub fn models_url(&self) -> Option<String> {
        let base = self.base_url()?;
        match self.models_path.as_deref() {
            Some("") => None,
            Some(path) if path.starts_with("http://") || path.starts_with("https://") => {
                Some(path.to_string())
            }
            Some(path) => Some(format!("{base}/{}", path.trim_start_matches('/'))),
            None => Some(format!("{base}/models")),
        }
    }

    pub fn resolve_headers(&self) -> Result<BTreeMap<String, String>, SecretError> {
        let mut out = BTreeMap::new();
        for (k, v) in &self.headers {
            out.insert(k.clone(), secret::resolve(v)?);
        }
        Ok(out)
    }
}

pub fn default_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("ANVIL_CONFIG") {
        return PathBuf::from(path);
    }
    crate::dirs_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/anvil/config.yaml")
}

pub fn default_cache_dir() -> PathBuf {
    if let Ok(path) = std::env::var("ANVIL_CACHE") {
        return PathBuf::from(path);
    }
    crate::dirs_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache/anvil")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
default_provider: omni
default_model: grok-4.5
providers:
  omni:
    base_url: http://127.0.0.1:20128/v1
    auth:
      type: api_key
      key: "!doppler secrets get LINKEDIN_OMNIROUTE_API_KEY -p roger -c dev_personal --plain"
  grok:
    base_url: https://api.x.ai/v1
    auth:
      type: oauth
      vendor: grok
"#;

    #[test]
    fn parses_named_providers() {
        let cfg: Config = serde_yaml::from_str(SAMPLE).unwrap();
        assert_eq!(cfg.default_provider.as_deref(), Some("omni"));
        let (_, omni) = cfg.provider(None).unwrap();
        assert_eq!(
            omni.base_url().as_deref(),
            Some("http://127.0.0.1:20128/v1")
        );
        match &omni.auth {
            Auth::ApiKey { key } => assert!(key.starts_with('!')),
            other => panic!("{other:?}"),
        }
        let (_, grok) = cfg.provider(Some("grok")).unwrap();
        match &grok.auth {
            Auth::Oauth {
                vendor: OauthVendor::Grok,
            } => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(
            grok.models_url().as_deref(),
            Some("https://api.x.ai/v1/models")
        );
    }
}
