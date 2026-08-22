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

/// A local HTTP server the native TUI exposes. Every path is config.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpDoor {
    /// Extra argv, `{port}` replaced.
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
    /// `{id}` is the session id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
}

impl HttpDoor {
    pub fn bind_argv(&self, port: u16) -> Option<String> {
        self.bind
            .as_deref()
            .map(|tmpl| tmpl.replace("{port}", &port.to_string()))
    }
}

/// Session files under `$HOME/<home>` for a title on an inhibit door.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionFiles {
    pub home: String,
    #[serde(default = "active_default")]
    pub active: String,
    #[serde(default = "summary_default")]
    pub summary: String,
    #[serde(default = "history_default")]
    pub history: String,
    #[serde(default)]
    pub title_keys: Vec<String>,
    #[serde(default)]
    pub strip_tags: Vec<String>,
}

fn active_default() -> String {
    "active_sessions.json".into()
}
fn summary_default() -> String {
    "summary.json".into()
}
fn history_default() -> String {
    "chat_history.jsonl".into()
}

/// A descendant cmdline means a turn is in flight.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InhibitDoor {
    /// Every string must appear in some descendant's cmdline.
    #[serde(default)]
    pub contains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<SessionFiles>,
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
    /// Extra argv to reopen one inner session. `{session}` is that
    /// pane's id — not a global continue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
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
        self.tui_spawn_session(None)
    }

    /// Native argv that reopens `session` when the row has `resume`.
    pub fn tui_spawn_session(&self, session: Option<&str>) -> (String, Option<String>) {
        let mut words: Vec<&str> = self.program.split_whitespace().collect();
        if words.last() == Some(&"acp") {
            words.pop();
        }
        let mut head = if words.is_empty() {
            self.program.clone()
        } else {
            words.join(" ")
        };
        if let Some(id) = session.filter(|s| !s.is_empty()) {
            if let Some(tmpl) = self.resume.as_deref() {
                let flags = tmpl.replace("{session}", id);
                if !flags.trim().is_empty() {
                    head = format!("{head} {flags}").trim().to_string();
                }
            }
        }
        if let Door::Http(http) = self.door() {
            if let Ok(port) = free_port() {
                if let Some(flags) = http.bind_argv(port) {
                    return (
                        format!("{head} {flags}").trim().to_string(),
                        Some(format!("http://127.0.0.1:{port}")),
                    );
                }
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
        serde_json::from_str(include_str!("../agents.default.json"))
            .expect("agents.default.json is valid catalog")
    }
}

impl Agents {
    pub fn load(root: &Path) -> Agents {
        Self::load_from(root).unwrap_or_else(|_| Agents::default())
    }

    /// Load the catalog. Writes the shipped file only when missing.
    /// A file that will not parse is an error — it is not overwritten.
    pub fn load_from(root: &Path) -> std::io::Result<Agents> {
        let path = root.join("agents.json");
        if !path.is_file() {
            let agents = Agents::default();
            agents.save(root)?;
            return Ok(agents);
        }
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text).map_err(std::io::Error::other)
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(root)?;
        let path = root.join("agents.json");
        let tmp = root.join("agents.json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn upsert(&mut self, agent: Agent) {
        if let Some(slot) = self.agents.iter_mut().find(|a| a.name == agent.name) {
            *slot = agent;
        } else {
            self.agents.push(agent);
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let n = self.agents.len();
        self.agents.retain(|a| a.name != name);
        if self.default == name {
            self.default = self
                .agents
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_default();
        }
        self.agents.len() < n
    }

    pub fn default_agent(&self) -> Agent {
        self.by_name(&self.default)
            .or_else(|| self.agents.first())
            .cloned()
            .unwrap_or_default()
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
        let _ = self.save(root);
    }

    /// HTTP door copied from the shipped catalog — config, not a brand.
    pub fn shipped_http() -> HttpDoor {
        Agents::default()
            .agents
            .iter()
            .find_map(|a| a.door().http().cloned())
            .unwrap_or_default()
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
        let shipped = Agents::default()
            .by_name("oc")
            .unwrap()
            .tui_spawn();
        assert!(shipped.0.contains("--port "));
        assert!(shipped.1.unwrap().starts_with("http://127.0.0.1:"));
    }

    #[test]
    fn resume_names_this_session_not_a_continue() {
        let oc = Agents::default().by_name("oc").unwrap().clone();
        assert_eq!(oc.resume.as_deref(), Some("--session {session}"));
        let (cmd, _) = oc.tui_spawn_session(Some("ses_pane_1"));
        assert!(cmd.contains("--session ses_pane_1"), "{cmd}");
        assert!(!cmd.contains("--continue"), "{cmd}");
        let (fresh, _) = oc.tui_spawn();
        assert!(!fresh.contains("--session"), "{fresh}");
        let (empty, _) = oc.tui_spawn_session(Some(""));
        assert!(!empty.contains("--session"), "{empty}");
        let grok = Agents::default().by_name("grok").unwrap().clone();
        let (cmd, _) = grok.tui_spawn_session(Some("ses_pane_2"));
        assert!(cmd.contains("--resume ses_pane_2"), "{cmd}");
        assert!(!cmd.contains("--continue"), "{cmd}");
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

    #[test]
    fn save_round_trips_upsert_and_remove() {
        let dir = std::env::temp_dir().join(format!("anvil-cat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut agents = Agents::default();
        agents.upsert(Agent {
            name: "claude".into(),
            program: "claude".into(),
            acp_program: Some("claude --acp".into()),
            adopt: vec!["claude".into()],
            ..Default::default()
        });
        agents.save(&dir).unwrap();
        let loaded = Agents::load(&dir);
        assert!(loaded.by_name("claude").is_some());
        let mut loaded = loaded;
        assert!(loaded.remove("claude"));
        loaded.save(&dir).unwrap();
        assert!(Agents::load(&dir).by_name("claude").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
