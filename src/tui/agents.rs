//! The agent's catalog: names and the programs the host spawns.
//! Lives at `<root>/agents.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How to seat an agent that has both a native TUI and ACP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// Their own TUI on a PTY.
    Native,
    /// Anvil's prompt/response viewer over ACP stdio.
    Anvil,
}

impl Seat {
    pub fn label(self) -> &'static str {
        match self {
            Seat::Native => "native TUI",
            Seat::Anvil => "anvil",
        }
    }
}

/// One named agent. Every agent is ACP-capable so the rail can see a
/// turn. `acp_only` is the inverse: no native TUI, so the seat is
/// always anvil. Otherwise the operator picks native or anvil at launch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    /// Native TUI, or the ACP program when `acp_only`.
    pub program: String,
    /// `"http"` — OpenCode's `/session/status` door on a native seat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<String>,
    /// No native TUI. Old catalogs used `"acp": true` for this.
    #[serde(default, alias = "acp", skip_serializing_if = "is_false")]
    pub acp_only: bool,
    /// ACP stdio program when the native TUI is a different command.
    /// Unused when `acp_only` — `program` is already ACP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_program: Option<String>,
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
                    acp_only: false,
                    acp_program: Some("oc acp".into()),
                },
                Agent {
                    name: "oc-work".into(),
                    program: "oc-work".into(),
                    watch: Some("http".into()),
                    acp_only: false,
                    acp_program: Some("oc-work acp".into()),
                },
                Agent {
                    name: "grok".into(),
                    program: "grok".into(),
                    watch: None,
                    acp_only: false,
                    acp_program: None,
                },
                Agent {
                    name: "rung".into(),
                    program: "rung-agent --acp".into(),
                    watch: None,
                    acp_only: true,
                    acp_program: None,
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
            acp_only: false,
            acp_program: Some("oc acp".into()),
        }
    }
}

impl Agent {
    pub fn seats(&self) -> Vec<Seat> {
        let mut out = Vec::new();
        if !self.acp_only {
            out.push(Seat::Native);
        }
        if self.acp_cmd().is_some() {
            out.push(Seat::Anvil);
        }
        if out.is_empty() {
            out.push(Seat::Native);
        }
        out
    }

    /// ACP stdio command, if this agent can sit in anvil's viewer.
    pub fn acp_cmd(&self) -> Option<&str> {
        if self.acp_only {
            Some(self.program.as_str())
        } else {
            self.acp_program.as_deref()
        }
    }

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

    fn oc() -> Agent {
        Agent {
            name: "oc".into(),
            program: "oc".into(),
            watch: Some("http".into()),
            acp_only: false,
            acp_program: Some("oc acp".into()),
        }
    }

    #[test]
    fn load_writes_the_default_file() {
        let dir = std::env::temp_dir().join(format!("anvil-agents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agents = Agents::load(&dir);
        assert_eq!(agents.default, "oc");
        let rung = agents.agents.iter().find(|a| a.name == "rung").unwrap();
        assert!(rung.acp_only);
        assert_eq!(rung.seats(), vec![Seat::Anvil]);
        let oc = agents.agents.iter().find(|a| a.name == "oc").unwrap();
        assert_eq!(oc.seats(), vec![Seat::Native, Seat::Anvil]);
        assert!(dir.join("agents.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_acp_true_is_acp_only() {
        let agent: Agent = serde_json::from_str(
            r#"{"name":"rung","program":"rung-agent --acp","acp":true}"#,
        )
        .unwrap();
        assert!(agent.acp_only);
        assert_eq!(agent.acp_cmd(), Some("rung-agent --acp"));
        assert_eq!(agent.seats(), vec![Seat::Anvil]);
    }

    #[test]
    fn grok_is_native_until_it_has_an_acp_program() {
        let grok = Agent {
            name: "grok".into(),
            program: "grok".into(),
            watch: None,
            acp_only: false,
            acp_program: None,
        };
        assert_eq!(grok.seats(), vec![Seat::Native]);
        assert_eq!(grok.acp_cmd(), None);
    }

    #[test]
    fn tui_spawn_keeps_the_wrapper() {
        let oc = oc();
        let (cmd, watch) = oc.tui_spawn();
        assert!(cmd.starts_with("oc --hostname 127.0.0.1 --port "));
        assert!(watch.unwrap().starts_with("http://127.0.0.1:"));

        let work = Agent {
            name: "oc-work".into(),
            program: "oc-work".into(),
            watch: Some("http".into()),
            acp_only: false,
            acp_program: Some("oc-work acp".into()),
        };
        let (cmd, _) = work.tui_spawn();
        assert!(cmd.starts_with("oc-work --hostname 127.0.0.1 --port "));
    }
}
