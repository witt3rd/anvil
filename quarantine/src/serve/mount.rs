//! Temporary mounts. In memory. Same trust as a strike. Unmount is total.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::inspect::Fiber;

pub struct Seat {
    pub occupant: String,
    pub text: Option<String>,
}

struct Live {
    kind: String,
    slot: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

pub struct Mounts {
    next: AtomicU64,
    live: Mutex<HashMap<String, Live>>,
    seats: Mutex<HashMap<String, Seat>>,
}

impl Default for Mounts {
    fn default() -> Self {
        Self {
            next: AtomicU64::new(1),
            live: Mutex::new(HashMap::new()),
            seats: Mutex::new(HashMap::new()),
        }
    }
}

impl Mounts {
    pub fn mount(self: &Arc<Self>, kind: &str, slot: &str) -> Result<String, String> {
        let kind = kind.trim();
        if kind != "clock" {
            return Err(format!(
                "unknown mount '{kind}': first toy is clock (occupies {slot})"
            ));
        }
        {
            let seats = self.seats.lock().map_err(|_| "seats".to_string())?;
            if seats.contains_key(slot) {
                return Err(format!("slot '{slot}' is occupied"));
            }
        }
        let id = format!("dyn-{}", self.next.fetch_add(1, Ordering::Relaxed));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut map = self.seats.lock().map_err(|_| "seats".to_string())?;
            map.insert(
                slot.to_string(),
                Seat {
                    occupant: id.clone(),
                    text: Some(clock_now()),
                },
            );
        }
        let stop_t = stop.clone();
        let slot_t = slot.to_string();
        let mounts = Arc::clone(self);
        let thread = thread::Builder::new()
            .name(format!("anvil-{id}"))
            .spawn(move || {
                while !stop_t.load(Ordering::Relaxed) {
                    if let Ok(mut map) = mounts.seats.lock() {
                        if let Some(seat) = map.get_mut(&slot_t) {
                            seat.text = Some(clock_now());
                        }
                    }
                    thread::sleep(Duration::from_millis(250));
                }
            })
            .map_err(|e| e.to_string())?;
        self.live.lock().map_err(|_| "live".to_string())?.insert(
            id.clone(),
            Live {
                kind: kind.into(),
                slot: slot.to_string(),
                stop,
                thread: Some(thread),
            },
        );
        Ok(id)
    }

    pub fn unmount(&self, id: &str) -> Result<(String, String), String> {
        let mut live = self.live.lock().map_err(|_| "live".to_string())?;
        let mut m = live
            .remove(id)
            .ok_or_else(|| format!("unknown mount '{id}'"))?;
        m.stop.store(true, Ordering::Relaxed);
        if let Some(t) = m.thread.take() {
            let _ = t.join();
        }
        drop(live);
        let mut seats = self.seats.lock().map_err(|_| "seats".to_string())?;
        if seats.get(&m.slot).map(|s| s.occupant.as_str()) == Some(id) {
            seats.remove(&m.slot);
        }
        Ok((m.kind, m.slot))
    }

    pub fn seat(&self, name: &str) -> Option<Seat> {
        self.seats.lock().ok().and_then(|m| {
            m.get(name).map(|s| Seat {
                occupant: s.occupant.clone(),
                text: s.text.clone(),
            })
        })
    }

    pub fn fibers(&self) -> Vec<Fiber> {
        self.live
            .lock()
            .map(|m| {
                m.iter()
                    .map(|(id, live)| Fiber {
                        name: format!("mount/{id}"),
                        kind: live.kind.clone(),
                        state: "active".into(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn clock_now() -> String {
    std::process::Command::new("date")
        .arg("+%H:%M:%S")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "--:--:--".into())
}
