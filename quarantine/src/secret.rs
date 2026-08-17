//! Resolve config strings. Same contract as Prime's `!command` keys, without
//! treating a bare word as an env name (that silently swaps a literal key).
//!
//! - `!rest` — run `rest` in the user's shell, use trimmed stdout
//! - `$NAME` / `${NAME}` — environment
//! - anything else — literal

use std::process::Command;
use std::sync::{Mutex, OnceLock};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("empty config value")]
    Empty,
    #[error("environment variable {0} is unset or empty")]
    MissingEnv(String),
    #[error("command `{command}` failed: {detail}")]
    Command { command: String, detail: String },
}

static COMMAND_CACHE: OnceLock<Mutex<std::collections::HashMap<String, String>>> = OnceLock::new();

fn cache() -> &'static Mutex<std::collections::HashMap<String, String>> {
    COMMAND_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

pub fn resolve(raw: &str) -> Result<String, SecretError> {
    resolve_inner(raw, true)
}

pub fn resolve_uncached(raw: &str) -> Result<String, SecretError> {
    resolve_inner(raw, false)
}

fn resolve_inner(raw: &str, use_cache: bool) -> Result<String, SecretError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SecretError::Empty);
    }
    if let Some(command) = trimmed.strip_prefix('!') {
        return run_command(command.trim(), use_cache);
    }
    if let Some(name) = env_name(trimmed) {
        return std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| SecretError::MissingEnv(name.to_string()));
    }
    Ok(trimmed.to_string())
}

fn env_name(raw: &str) -> Option<&str> {
    if let Some(inner) = raw.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
        return Some(inner);
    }
    raw.strip_prefix('$').filter(|name| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn run_command(command: &str, use_cache: bool) -> Result<String, SecretError> {
    if command.is_empty() {
        return Err(SecretError::Command {
            command: String::new(),
            detail: "empty command after !".into(),
        });
    }
    if use_cache {
        if let Ok(guard) = cache().lock() {
            if let Some(hit) = guard.get(command) {
                return Ok(hit.clone());
            }
        }
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| SecretError::Command {
            command: command.to_string(),
            detail: err.to_string(),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SecretError::Command {
            command: command.to_string(),
            detail: format!(
                "exit {}{}",
                output.status.code().unwrap_or(-1),
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", stderr.trim())
                }
            ),
        });
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        return Err(SecretError::Command {
            command: command.to_string(),
            detail: "stdout was empty".into(),
        });
    }
    if use_cache {
        if let Ok(mut guard) = cache().lock() {
            guard.insert(command.to_string(), value.clone());
        }
    }
    Ok(value)
}

#[cfg(test)]
pub fn clear_command_cache() {
    if let Ok(mut guard) = cache().lock() {
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_is_unchanged() {
        assert_eq!(resolve("sk-live").unwrap(), "sk-live");
    }

    #[test]
    fn dollar_env() {
        std::env::set_var("ANVIL_TEST_SECRET", "from-env");
        assert_eq!(resolve("$ANVIL_TEST_SECRET").unwrap(), "from-env");
        assert_eq!(resolve("${ANVIL_TEST_SECRET}").unwrap(), "from-env");
        std::env::remove_var("ANVIL_TEST_SECRET");
        assert!(matches!(
            resolve("$ANVIL_TEST_SECRET"),
            Err(SecretError::MissingEnv(_))
        ));
    }

    #[test]
    fn bang_runs_command() {
        clear_command_cache();
        assert_eq!(resolve("!printf 'doppler-out'").unwrap(), "doppler-out");
    }

    #[test]
    fn bang_failure_is_loud() {
        clear_command_cache();
        let err = resolve("!exit 7").unwrap_err();
        match err {
            SecretError::Command { detail, .. } => assert!(detail.contains("7"), "{detail}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bare_word_is_not_an_env_lookup() {
        std::env::set_var("sk-looks-like-a-key", "nope");
        assert_eq!(
            resolve("sk-looks-like-a-key").unwrap(),
            "sk-looks-like-a-key"
        );
        std::env::remove_var("sk-looks-like-a-key");
    }
}
