// As a user: run `anvil` to become the client, attaching to a session.
// The session persists on disk (kernel: "Sessions, windows, and panes stay").
// Allocate a window -> pane -> process (e.g., nvim).
// Each pane holds a PTY; the process runs on the slave PTY.
// On reattach, the client repaints from the daemon's character grid.
use std::io;
use std::path::PathBuf;

use anvil::daemon;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "anvil")]
#[command(about = "Terminal multiplexer", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// The daemon: owns sessions, serves clients over a unix socket.
    Daemon {
        /// Override the socket path (`ANVIL_SOCK`).
        #[arg(long)]
        sock: Option<PathBuf>,
        /// Override the state root (`~/.anvil` by default).
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Daemon { sock, root }) => {
            let sock = sock.unwrap_or_else(daemon::default_sock);
            let root = root.unwrap_or_else(|| std::env::var("ANVIL_ROOT").unwrap_or_else(|_| default_root()).into());
            daemon::run(root, sock)
        }
        None => {
            let sock = daemon::default_sock();
            let root = std::env::var("ANVIL_ROOT").unwrap_or_else(|_| default_root());
            daemon::ensure_running(&sock, std::path::Path::new(&root))?;
            anvil::tui::run(&sock)
        }
    }
}

fn default_root() -> String {
    std::env::var("HOME").map(|h| format!("{h}/.anvil")).unwrap_or_else(|_| ".anvil".into())
}
