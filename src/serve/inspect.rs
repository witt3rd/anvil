//! Live report: fibers, services, slots. Catalog ∩ running store.

use serde::{Deserialize, Serialize};

use super::State;

pub const SLOT_RAIL: &str = "casing.rail";
pub const SLOT_MAIN: &str = "casing.main";
pub const SLOT_STATUS: &str = "casing.status";
pub const SLOT_TRANSCRIPT: &str = "session.transcript";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Slot {
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Fiber {
    pub name: String,
    pub kind: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub kind: String,
    pub state: String,
    pub events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub root: String,
    pub sock: String,
    pub services: Vec<Service>,
    pub fibers: Vec<Fiber>,
    pub slots: Vec<Slot>,
    pub workspaces: Vec<String>,
    pub catalogs: Vec<String>,
    #[serde(default)]
    pub prof: crate::prof::Snap,
}

impl State {
    pub fn inspect(&self) -> Report {
        let sessions = self.root.list_sessions().unwrap_or_default();
        let hot = self
            .slots
            .lock()
            .map(|m| m.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut services = Vec::new();
        let mut fibers = Vec::new();
        for sess in &sessions {
            let events = self
                .root
                .load_events(&sess.id)
                .map(|e| e.len() as u64)
                .unwrap_or(0);
            let state = if hot.iter().any(|h| h == &sess.id) {
                "hot"
            } else {
                "cold"
            };
            services.push(Service {
                name: sess.id.clone(),
                kind: "session".into(),
                state: state.into(),
                events,
            });
            fibers.push(Fiber {
                name: format!("adapter/{}", sess.id),
                kind: "adapter".into(),
                state: if state == "hot" { "active" } else { "pending" }.into(),
            });
        }
        fibers.extend(self.mounts.fibers());
        let mut seen_pty = Vec::new();
        for ws in self.root.list_workspaces().unwrap_or_default() {
            for member in ws.members {
                if !member.is_pty() {
                    continue;
                }
                let name = member.id().to_string();
                if seen_pty.iter().any(|n| n == &name) {
                    continue;
                }
                seen_pty.push(name.clone());
                let hot = self.ptys.is_hot(&name);
                services.push(Service {
                    name: name.clone(),
                    kind: "pty".into(),
                    state: if hot { "hot".into() } else { "cold".into() },
                    events: 0,
                });
                fibers.push(Fiber {
                    name: format!("adapter/{name}"),
                    kind: "pty".into(),
                    state: if hot { "active" } else { "pending" }.into(),
                });
            }
        }
        for name in self.ptys.names() {
            if seen_pty.iter().any(|n| n == &name) {
                continue;
            }
            let hot = self.ptys.is_hot(&name);
            services.push(Service {
                name: name.clone(),
                kind: "pty".into(),
                state: if hot { "hot".into() } else { "cold".into() },
                events: 0,
            });
            fibers.push(Fiber {
                name: format!("adapter/{name}"),
                kind: "pty".into(),
                state: if hot { "active" } else { "pending" }.into(),
            });
        }
        let mut seen_edit = Vec::new();
        for ws in self.root.list_workspaces().unwrap_or_default() {
            for member in ws.members {
                if !member.is_edit() {
                    continue;
                }
                let name = member.id().to_string();
                if seen_edit.iter().any(|n| n == &name) {
                    continue;
                }
                seen_edit.push(name.clone());
                let hot = self.edits.is_hot(&name);
                services.push(Service {
                    name: name.clone(),
                    kind: "edit".into(),
                    state: if hot { "hot".into() } else { "cold".into() },
                    events: 0,
                });
                fibers.push(Fiber {
                    name: format!("adapter/{name}"),
                    kind: "edit".into(),
                    state: if hot { "active" } else { "pending" }.into(),
                });
            }
        }
        let front = self.live_front();
        let front_text = front.as_deref().and_then(|name| {
            self.ptys.peek(name).and_then(|s| s.preview()).or_else(|| {
                self.edits.snap(name).ok().and_then(|b| {
                    b.text
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .map(str::to_string)
                })
            })
        });
        let status = self.mounts.seat(SLOT_STATUS);
        let slots = vec![
            Slot {
                name: SLOT_RAIL.into(),
                kind: "chrome".into(),
                occupant: None,
                text: None,
            },
            Slot {
                name: SLOT_MAIN.into(),
                kind: "stage".into(),
                occupant: front.clone(),
                text: front_text,
            },
            Slot {
                name: SLOT_STATUS.into(),
                kind: "chrome".into(),
                occupant: status.as_ref().map(|s| s.occupant.clone()),
                text: status.and_then(|s| s.text),
            },
            Slot {
                name: SLOT_TRANSCRIPT.into(),
                kind: "smith".into(),
                occupant: front,
                text: None,
            },
        ];
        Report {
            root: self.root.root().display().to_string(),
            sock: self.sock.display().to_string(),
            services,
            fibers,
            slots,
            workspaces: self
                .root
                .list_workspaces()
                .unwrap_or_default()
                .into_iter()
                .map(|w| w.name)
                .collect(),
            catalogs: self
                .root
                .list_catalogs()
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.name)
                .collect(),
            prof: crate::prof::snapshot(),
        }
    }
}
