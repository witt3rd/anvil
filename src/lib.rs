//! anvil: a terminal multiplexer. The daemon owns sessions and serves
//! clients over a unix socket. One binary, two seats: the client is
//! the default seat; `anvil daemon` is the daemon.

pub mod catalog;
pub mod catalog_cmd;
pub mod daemon;
pub mod fd;
pub mod proto;
pub mod tui;

/// Git of this ELF. The daemon puts it on `enumerate`. The client
/// will not attach to another build.
pub fn build_id() -> &'static str {
    option_env!("ANVIL_BUILD").unwrap_or("unknown")
}
