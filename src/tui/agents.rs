//! The agent's catalog: names and the programs the host spawns.
//! Lives at `<root>/agents.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One named ACP program.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    pub program: String,
}

/// The catalog and which name is the default.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agents {
    pub default: String,
    pub agents: Vec<Agent>,
}

impl Default for Agents {
    fn default() -> Self {
        Agents {
            default: "opencode".into(),
            agents: vec![
                Agent {
                    name: "opencode".into(),
                    program: "opencode acp".into(),
                },
                Agent {
                    name: "grok".into(),
                    program: "grok".into(),
                },
            ],
        }
    }
}

impl Agents {
    /// Load `<root>/agents.json`. Write the default file when missing.
    pub fn load(root: &Path) -> Agents {
        let path = root.join("agents.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(agents) = serde_json::from_str(&text) {
                return agents;
            }
        }
        let agents = Agents::default();
        let _ = std::fs::create_dir_all(root);
        if let Ok(text) = serde_json::to_string_pretty(&agents) {
            let _ = std::fs::write(path, text);
        }
        agents
    }

    pub fn default_agent(&self) -> Agent {
        self.by_name(&self.default)
            .or_else(|| self.agents.first())
            .cloned()
            .unwrap_or_else(Agents::fallback)
    }

    pub fn by_name(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.name == name)
    }

    /// The program for the default agent. `ANVIL_ACP` wins.
    pub fn default_program(&self) -> String {
        std::env::var("ANVIL_ACP").unwrap_or_else(|_| self.default_agent().program.clone())
    }

    /// Remember `name` as the default and write `agents.json`.
    pub fn set_default(&mut self, name: &str, root: &Path) {
        if self.by_name(name).is_none() {
            return;
        }
        self.default = name.to_string();
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(root.join("agents.json"), text);
        }
    }
}

impl Agents {
    fn fallback() -> Agent {
        Agent {
            name: "opencode".into(),
            program: "opencode acp".into(),
        }
    }
}

/// A window name that is not yet taken: `opencode`, then `opencode-2`.
pub fn unique_name(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !taken.iter().any(|t| t == &candidate) {
            return candidate;
        }
        n += 1;
    }
}

pub fn default_root() -> PathBuf {
    if let Ok(root) = std::env::var("ANVIL_ROOT") {
        return PathBuf::from(root);
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".anvil"))
        .unwrap_or_else(|_| PathBuf::from(".anvil"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_name_counts_up() {
        assert_eq!(unique_name("opencode", &[]), "opencode");
        assert_eq!(
            unique_name("opencode", &["opencode".into()]),
            "opencode-2"
        );
        assert_eq!(
            unique_name("opencode", &["opencode".into(), "opencode-2".into()]),
            "opencode-3"
        );
    }

    #[test]
    fn load_writes_the_default_file() {
        let dir = std::env::temp_dir().join(format!("anvil-agents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agents = Agents::load(&dir);
        assert_eq!(agents.default, "opencode");
        assert!(dir.join("agents.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
