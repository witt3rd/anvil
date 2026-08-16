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
        let front = self.live_front();
        let slots = vec![
            Slot {
                name: SLOT_RAIL.into(),
                kind: "chrome".into(),
                occupant: None,
            },
            Slot {
                name: SLOT_MAIN.into(),
                kind: "stage".into(),
                occupant: front.clone(),
            },
            Slot {
                name: SLOT_STATUS.into(),
                kind: "chrome".into(),
                occupant: None,
            },
            Slot {
                name: SLOT_TRANSCRIPT.into(),
                kind: "smith".into(),
                occupant: front,
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
        }
    }
}
