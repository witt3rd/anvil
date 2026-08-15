//! Vendor OAuth. We do not implement OAuth. We run the vendor's login and
//! read their cache — the grok-build exemplar from jcode.

use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;

use crate::config::OauthVendor;

#[derive(Debug, Error)]
pub enum OauthError {
    #[error("failed to launch {bin}: {source}")]
    Spawn {
        bin: String,
        #[source]
        source: std::io::Error,
    },
    #[error("`{0} login` exited {1}")]
    Failed(String, String),
}

pub fn login(vendor: OauthVendor) -> Result<(), OauthError> {
    match vendor {
        OauthVendor::Grok => run_login("grok"),
    }
}

pub fn has_login(vendor: OauthVendor) -> bool {
    match vendor {
        OauthVendor::Grok => grok_has_login(),
    }
}

/// Bearer token if the vendor cache has one. Never log this.
pub fn cached_token(vendor: OauthVendor) -> Option<String> {
    match vendor {
        OauthVendor::Grok => grok_cached_token(),
    }
}

pub fn grok_auth_path() -> Option<PathBuf> {
    crate::dirs_home().map(|home| home.join(".grok/auth.json"))
}

fn grok_has_login() -> bool {
    if std::env::var("GROK_DEPLOYMENT_KEY")
        .ok()
        .is_some_and(|v| !v.trim().is_empty())
    {
        return true;
    }
    grok_cached_token().is_some()
}

fn grok_cached_token() -> Option<String> {
    if let Ok(key) = std::env::var("GROK_DEPLOYMENT_KEY") {
        let key = key.trim();
        if !key.is_empty() {
            return Some(key.to_string());
        }
    }
    let path = grok_auth_path()?;
    let bytes = std::fs::read(path).ok()?;
    grok_token_from_bytes(&bytes)
}

fn grok_token_from_bytes(bytes: &[u8]) -> Option<String> {
    let serde_json::Value::Object(scopes) = serde_json::from_slice(bytes).ok()? else {
        return None;
    };
    scopes.values().find_map(|cred| {
        cred.get("key")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(ToString::to_string)
    })
}

fn run_login(bin: &str) -> Result<(), OauthError> {
    let status = Command::new(bin)
        .arg("login")
        .status()
        .map_err(|source| OauthError::Spawn {
            bin: bin.to_string(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(OauthError::Failed(bin.to_string(), status.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_auth_json_requires_a_nonempty_key() {
        assert!(grok_token_from_bytes(br#"{}"#).is_none());
        assert!(grok_token_from_bytes(br#"{"https://auth.x.ai::client":{"key":""}}"#).is_none());
        assert_eq!(
            grok_token_from_bytes(br#"{"https://auth.x.ai::client":{"key":"tok"}}"#).as_deref(),
            Some("tok")
        );
    }
}
