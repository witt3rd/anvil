//! The adapter for a named agent. The rest of anvil knows spawn,
//! adopt, and a door — not OpenCode, grok, or the flavor of the week.
//!
//! Adding support is a row in `<root>/agents.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How to seat an agent that has both a native TUI and ACP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    Native,
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

/// A local HTTP server the native TUI exposes. Paths are whatever
/// that TUI documents; empty fields use the defaults below.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpDoor {
    /// Extra argv, `{port}` replaced. Default binds 127.0.0.1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit: Option<String>,
}

impl HttpDoor {
    pub fn bind_argv(&self, port: u16) -> String {
        let tmpl = self
            .bind
            .as_deref()
            .unwrap_or("--hostname 127.0.0.1 --port {port}");
        tmpl.replace("{port}", &port.to_string())
    }

    pub fn health(&self) -> &str {
        self.health.as_deref().unwrap_or("/global/health")
    }

    pub fn status(&self) -> &str {
        self.status.as_deref().unwrap_or("/session/status")
    }

    pub fn sessions(&self) -> &str {
        self.sessions.as_deref().unwrap_or("/session")
    }

    pub fn append(&self) -> &str {
        self.append.as_deref().unwrap_or("/tui/append-prompt")
    }

    pub fn submit(&self) -> &str {
        self.submit.as_deref().unwrap_or("/tui/submit-prompt")
    }
}

impl Default for HttpDoor {
    fn default() -> Self {
        HttpDoor {
            bind: None,
            health: None,
            status: None,
            sessions: None,
            append: None,
            submit: None,
        }
    }
}

/// A descendant cmdline means a turn is in flight. Optional `home`
/// is a directory under `$HOME` with `active_sessions.json` for a title.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InhibitDoor {
    /// Every string must appear in some descendant's cmdline.
    #[serde(default)]
    pub contains: Vec<String>,
    /// e.g. `".grok"` — title from that product's session files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
}

/// How the rail and the courier see a **native** seat.
/// ACP panes do not use this — ACP is already the protocol.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Door {
    #[default]
    None,
    Http(HttpDoor),
    Inhibit(InhibitDoor),
}

impl Door {
    pub fn is_http(&self) -> bool {
        matches!(self, Door::Http(_))
    }

    pub fn http(&self) -> Option<&HttpDoor> {
        match self {
            Door::Http(h) => Some(h),
            _ => None,
        }
    }

    pub fn inhibit(&self) -> Option<&InhibitDoor> {
        match self {
            Door::Inhibit(i) => Some(i),
            _ => None,
        }
    }
}

/// One named agent — one adapter.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    /// Native TUI, or the ACP program when `acp_only`.
    pub program: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<String>,
    #[serde(default, alias = "acp", skip_serializing_if = "is_false")]
    pub acp_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_program: Option<String>,
    /// Basenames that mean this row when found in a shell's tree.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adopt: Vec<String>,
    #[serde(default, skip_serializing_if = "door_is_none")]
    pub door: Door,
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn door_is_none(d: &Door) -> bool {
    matches!(d, Door::None)
}

impl Agent {
    /// Resolved door: `door`, else old `"watch": "http"`.
    pub fn door(&self) -> Door {
        if !matches!(self.door, Door::None) {
            return self.door.clone();
        }
        if self.watch.as_deref() == Some("http") {
            return Door::Http(HttpDoor::default());
        }
        Door::None
    }

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

    pub fn acp_cmd(&self) -> Option<&str> {
        if self.acp_only {
            Some(self.program.as_str())
        } else {
            self.acp_program.as_deref()
        }
    }

    /// Native argv and optional HTTP base. `{port}` comes from a free bind.
    pub fn tui_spawn(&self) -> (String, Option<String>) {
        let mut words: Vec<&str> = self.program.split_whitespace().collect();
        if words.last() == Some(&"acp") {
            words.pop();
        }
        let head = if words.is_empty() {
            self.program.clone()
        } else {
            words.join(" ")
        };
        if let Door::Http(http) = self.door() {
            if let Ok(port) = free_port() {
                let flags = http.bind_argv(port);
                return (
                    format!("{head} {flags}").trim().to_string(),
                    Some(format!("http://127.0.0.1:{port}")),
                );
            }
        }
        (head, None)
    }

    fn adopt_names(&self) -> Vec<&str> {
        if !self.adopt.is_empty() {
            return self.adopt.iter().map(String::as_str).collect();
        }
        vec![self.name.as_str()]
    }
}

pub fn port_from_args(words: &[&str]) -> Option<u16> {
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if let Some(rest) = w.strip_prefix("--port=") {
            return rest.parse().ok();
        }
        if w == "--port" {
            return words.get(i + 1)?.parse().ok();
        }
        i += 1;
    }
    None
}

fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
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
                    watch: None,
                    acp_only: false,
                    acp_program: Some("oc acp".into()),
                    adopt: vec!["oc".into(), "opencode".into()],
                    door: Door::Http(HttpDoor::default()),
                },
                Agent {
                    name: "oc-work".into(),
                    program: "oc-work".into(),
                    watch: None,
                    acp_only: false,
                    acp_program: Some("oc-work acp".into()),
                    adopt: vec!["oc-work".into()],
                    door: Door::Http(HttpDoor::default()),
                },
                Agent {
                    name: "grok".into(),
                    program: "grok".into(),
                    watch: None,
                    acp_only: false,
                    acp_program: None,
                    adopt: vec!["grok".into()],
                    door: Door::Inhibit(InhibitDoor {
                        contains: vec![
                            "systemd-inhibit".into(),
                            "turn in progress".into(),
                        ],
                        home: Some(".grok".into()),
                    }),
                },
                Agent {
                    name: "rung".into(),
                    program: "rung-agent --acp".into(),
                    watch: None,
                    acp_only: true,
                    acp_program: None,
                    adopt: Vec::new(),
                    door: Door::None,
                },
            ],
        }
    }
}

impl Agents {
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

    pub fn default_program(&self) -> String {
        std::env::var("ANVIL_ACP").unwrap_or_else(|_| self.default_agent().program.clone())
    }

    pub fn set_default(&mut self, name: &str, root: &Path) {
        if self.by_name(name).is_none() {
            return;
        }
        self.default = name.to_string();
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(root.join("agents.json"), text);
        }
    }

    /// Longest adopt alias that appears as a basename in `cmd`.
    pub fn adopt_cmd(&self, cmd: &str) -> Option<AdoptHit> {
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let bins: Vec<&str> = words.iter().filter_map(|w| Path::new(w).file_name()?.to_str()).collect();
        let argv0 = bins.first().copied().unwrap_or("");
        let mut best: Option<(usize, AdoptHit)> = None;
        for agent in &self.agents {
            for alias in agent.adopt_names() {
                if !cmd_means(&bins, argv0, alias) {
                    continue;
                }
                let score = alias.len();
                let watch = port_from_args(&words).map(|p| format!("http://127.0.0.1:{p}"));
                let hit = AdoptHit {
                    name: agent.name.clone(),
                    watch,
                    listen: agent.door().is_http(),
                };
                if best.as_ref().is_none_or(|(s, _)| score > *s) {
                    best = Some((score, hit));
                }
            }
        }
        best.map(|(_, h)| h)
    }
}

fn cmd_means(bins: &[&str], argv0: &str, alias: &str) -> bool {
    if argv0 == alias {
        return true;
    }
    if (argv0 == "bash" || argv0 == "sh") && bins.get(1).copied() == Some(alias) {
        return true;
    }
    if argv0 == "node" && bins.iter().any(|b| *b == alias) {
        return true;
    }
    false
}

impl Agents {
    fn fallback() -> Agent {
        Agent {
            name: "oc".into(),
            program: "oc".into(),
            watch: None,
            acp_only: false,
            acp_program: Some("oc acp".into()),
            adopt: vec!["oc".into(), "opencode".into()],
            door: Door::Http(HttpDoor::default()),
        }
    }
}

/// A process tree matched a catalog row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptHit {
    pub name: String,
    pub watch: Option<String>,
    pub listen: bool,
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
    }

    #[test]
    fn door_http_from_old_watch_field() {
        let a: Agent = serde_json::from_str(
            r#"{"name":"oc","program":"oc","watch":"http","acp_program":"oc acp"}"#,
        )
        .unwrap();
        assert!(a.door().is_http());
        let (cmd, url) = a.tui_spawn();
        assert!(cmd.starts_with("oc --hostname 127.0.0.1 --port "));
        assert!(url.unwrap().starts_with("http://127.0.0.1:"));
    }

    #[test]
    fn adopt_prefers_the_longer_alias() {
        let agents = Agents::default();
        let hit = agents.adopt_cmd("/home/dt/.local/bin/oc-work").unwrap();
        assert_eq!(hit.name, "oc-work");
        let oc = agents.adopt_cmd("oc --port 9").unwrap();
        assert_eq!(oc.name, "oc");
        assert_eq!(oc.watch.as_deref(), Some("http://127.0.0.1:9"));
        let raw = agents.adopt_cmd("opencode").unwrap();
        assert_eq!(raw.name, "oc");
        assert!(agents.adopt_cmd("git clone grok").is_none());
        assert!(agents.adopt_cmd("sh").is_none());
        let wrap = agents.adopt_cmd("/bin/bash /home/dt/.local/bin/oc").unwrap();
        assert_eq!(wrap.name, "oc");
        let node = agents
            .adopt_cmd("node /x/node_modules/@xai-official/grok/bin/grok")
            .unwrap();
        assert_eq!(node.name, "grok");
    }

    #[test]
    fn acp_only_has_no_native_seat() {
        let rung = Agents::default()
            .agents
            .into_iter()
            .find(|a| a.name == "rung")
            .unwrap();
        assert_eq!(rung.seats(), vec![Seat::Anvil]);
    }
}
