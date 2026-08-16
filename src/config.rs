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
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub keys: KeysConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

/// Casing chrome. `copy_on_select` matches herdr (default on).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Drag-select copies and toasts. `false` keeps the highlight; Ctrl+C copies.
    pub copy_on_select: bool,
    /// Hide the casing status line except inside the focused session seat.
    /// Default off: the bar stays put like a TWM chrome strip.
    pub status_auto_hide: bool,
    /// Ordered status widgets. Unknown names are skipped.
    /// Built-ins: account, cwd, git, model, context, clock, focus, spin.
    /// `clock` reads the `casing.status` mount (the existing slot).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub status: Vec<String>,
    /// Assumed context window when the provider does not say. Used only
    /// for the context widget fill. None = no bar, just token count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            copy_on_select: true,
            status_auto_hide: false,
            status: Vec::new(),
            context_window: Some(128_000),
        }
    }
}

impl UiConfig {
    pub fn status_widgets(&self) -> Vec<String> {
        if self.status.is_empty() {
            vec![
                "account".into(),
                "cwd".into(),
                "git".into(),
                "model".into(),
                "context".into(),
                "clock".into(),
            ]
        } else {
            self.status.clone()
        }
    }
}

/// Herdr-style keymap. `prefix` plus named actions (`next_sash`, `detach`).
/// A value is one chord or a list: `prefix+n` or `[prefix+n, ctrl+alt+]]`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KeysConfig {
    pub prefix: Option<String>,
    #[serde(flatten)]
    pub actions: BTreeMap<String, KeySpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum KeySpec {
    #[default]
    Empty,
    One(String),
    Many(Vec<String>),
}

impl KeySpec {
    pub fn as_slice(&self) -> &[String] {
        match self {
            KeySpec::Empty => &[],
            KeySpec::One(s) => std::slice::from_ref(s),
            KeySpec::Many(v) => v,
        }
    }
}

/// Pack name plus optional ink/face overrides. Faces are dotted names
/// (`message.user.field`, `hint.key`) resolved by the TUI theme engine.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ThemeConfig {
    pub pack: Option<String>,
    #[serde(default)]
    pub ink: BTreeMap<String, String>,
    #[serde(default)]
    pub face: BTreeMap<String, ThemeFace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ThemeFace {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: Option<bool>,
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
        assert!(cfg.theme.pack.is_none());
        assert!(cfg.ui.copy_on_select);
    }

    #[test]
    fn copy_on_select_can_be_disabled() {
        let raw = format!("{SAMPLE}\nui:\n  copy_on_select: false\n");
        let cfg: Config = serde_yaml::from_str(&raw).unwrap();
        assert!(!cfg.ui.copy_on_select);
        assert!(!cfg.ui.status_auto_hide);
    }

    #[test]
    fn status_widgets_default_and_override() {
        let cfg: Config = serde_yaml::from_str(SAMPLE).unwrap();
        assert_eq!(
            cfg.ui.status_widgets(),
            vec!["account", "cwd", "git", "model", "context", "clock"]
        );
        let raw = format!("{SAMPLE}\nui:\n  status_auto_hide: true\n  status: [model, context]\n");
        let cfg: Config = serde_yaml::from_str(&raw).unwrap();
        assert!(cfg.ui.status_auto_hide);
        assert_eq!(cfg.ui.status_widgets(), vec!["model", "context"]);
    }

    #[test]
    fn theme_pack_and_overrides_parse() {
        let raw = r##"
default_provider: omni
providers:
  omni:
    base_url: http://x
    auth:
      type: api_key
      key: k
theme:
  pack: terminal
  ink:
    accent: "#ff00aa"
  face:
    hint.key:
      fg: accent
      bold: true
"##;
        let cfg: Config = serde_yaml::from_str(raw).unwrap();
        assert_eq!(cfg.theme.pack.as_deref(), Some("terminal"));
        assert_eq!(
            cfg.theme.ink.get("accent").map(String::as_str),
            Some("#ff00aa")
        );
        let face = cfg.theme.face.get("hint.key").expect("hint.key");
        assert_eq!(face.fg.as_deref(), Some("accent"));
        assert_eq!(face.bold, Some(true));
    }

    #[test]
    fn keys_prefix_and_list_parse() {
        let raw = r#"
default_provider: omni
providers:
  omni:
    base_url: http://x
    auth:
      type: api_key
      key: k
keys:
  prefix: ctrl+a
  next_sash:
    - prefix+n
    - ctrl+alt+]
  detach: prefix+q
"#;
        let cfg: Config = serde_yaml::from_str(raw).unwrap();
        assert_eq!(cfg.keys.prefix.as_deref(), Some("ctrl+a"));
        assert_eq!(
            cfg.keys.actions.get("detach").unwrap().as_slice(),
            ["prefix+q"]
        );
        assert_eq!(
            cfg.keys.actions.get("next_sash").unwrap().as_slice().len(),
            2
        );
    }
}
