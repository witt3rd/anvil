//! Heads waiting on a pane. A PTY byte (or ACP chunk) pings them so
//! the daemon paints — no frame clock.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

/// A message on a head's channel.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The pane's view changed.
    Wake,
    /// A key, mouse, or resize from the client's tty.
    Input(crate::proto::Input),
}

pub struct Wake {
    txs: Mutex<Vec<Sender<Msg>>>,
}

impl Wake {
    pub fn new() -> Wake {
        Wake {
            txs: Mutex::new(Vec::new()),
        }
    }

    pub fn register(&self) -> (Sender<Msg>, Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut txs) = self.txs.lock() {
            txs.push(tx.clone());
        }
        (tx, rx)
    }

    pub fn ping(&self) {
        let Ok(mut txs) = self.txs.lock() else {
            return;
        };
        txs.retain(|tx| tx.send(Msg::Wake).is_ok());
    }
}

impl Default for Wake {
    fn default() -> Self {
        Self::new()
    }
}
