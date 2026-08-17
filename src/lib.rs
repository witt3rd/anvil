//! anvil: a terminal multiplexer. The daemon owns sessions and serves
//! clients over a unix socket. One binary, two seats: the client is
//! the default seat; `anvil daemon` is the daemon.

pub mod daemon;
pub mod proto;
pub mod tui;