//! The agent's catalog: names and the programs the host spawns.
//! Lives at `<root>/agents.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One named agent: the program to spawn, and how the rail watches it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    pub program: String,
    /// `"http"` — OpenCode's `/session/status` door. The TUI is launched
    /// with `--hostname 127.0.0.1 --port N` so the wrapper stays argv0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<String>,
    /// The process speaks ACP on stdio. The pane is the prompt/response
    /// view, not a PTY.
    #[serde(default, skip_serializing_if = "is_false")]
    pub acp: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
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
            default: "oc".into(),
            agents: vec![
                Agent {
                    name: "oc".into(),
                    program: "oc".into(),
                    watch: Some("http".into()),
                    acp: false,
                },
                Agent {
                    name: "oc-work".into(),
                    program: "oc-work".into(),
                    watch: Some("http".into()),
                    acp: false,
                },
                Agent {
                    name: "grok".into(),
                    program: "grok".into(),
                    watch: None,
                    acp: false,
                },
                Agent {
                    name: "rung".into(),
                    program: "rung-agent --acp".into(),
                    watch: None,
                    acp: true,
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
            name: "oc".into(),
            program: "oc".into(),
            watch: Some("http".into()),
            acp: false,
        }
    }
}

impl Agent {
    /// TUI command and optional HTTP door for the rail.
    /// `watch: "http"` (or a known OpenCode wrapper) keeps `program` as
    /// argv0 and appends `--hostname 127.0.0.1 --port N`.
    pub fn tui_spawn(&self) -> (String, Option<String>) {
        let mut words: Vec<&str> = self.program.split_whitespace().collect();
        if words.last() == Some(&"acp") {
            words.pop();
        }
        let first = words.first().copied().unwrap_or("oc");
        if self.uses_http() {
            if let Ok(port) = free_port() {
                let head = if words.is_empty() {
                    first.to_string()
                } else {
                    words.join(" ")
                };
                return (
                    format!("{head} --hostname 127.0.0.1 --port {port}"),
                    Some(format!("http://127.0.0.1:{port}")),
                );
            }
        }
        let program = if words.is_empty() {
            first.to_string()
        } else {
            words.join(" ")
        };
        (program, None)
    }

    fn uses_http(&self) -> bool {
        if self.watch.as_deref() == Some("http") {
            return true;
        }
        let first = self.program.split_whitespace().next().unwrap_or("");
        matches!(
            (self.name.as_str(), first),
            ("opencode" | "oc" | "oc-work", _) | (_, "opencode" | "oc" | "oc-work")
        )
    }
}

fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
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
        assert_eq!(agents.default, "oc");
        assert!(agents.agents.iter().any(|a| a.name == "rung" && a.acp));
        assert!(dir.join("agents.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tui_spawn_keeps_the_wrapper() {
        let oc = Agent {
            name: "oc".into(),
            program: "oc".into(),
            watch: Some("http".into()),
            acp: false,
        };
        let (cmd, watch) = oc.tui_spawn();
        assert!(cmd.starts_with("oc --hostname 127.0.0.1 --port "));
        assert!(watch.unwrap().starts_with("http://127.0.0.1:"));

        let work = Agent {
            name: "oc-work".into(),
            program: "oc-work".into(),
            watch: Some("http".into()),
            acp: false,
        };
        let (cmd, _) = work.tui_spawn();
        assert!(cmd.starts_with("oc-work --hostname 127.0.0.1 --port "));
    }
}
