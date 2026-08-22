// No command: the client. `anvil daemon` stays up as parent of every
// process. Detach never kills. Prefix q detaches.
use std::io;
use std::path::PathBuf;

use anvil::daemon;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "anvil")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("ANVIL_BUILD"), ")"))]
#[command(about = "A multiplexer for ACP agents and shells")]
#[command(long_about = LONG_ABOUT)]
#[command(after_help = AFTER_HELP)]
struct Cli {
    /// Stop the daemon on this socket, start this binary, attach.
    #[arg(long)]
    restart: bool,
    /// State directory (`ANVIL_ROOT`, else `~/.anvil`).
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Stay up. Parent of every process. Owns sessions. Socket for clients.
    Daemon {
        /// Unix socket (`ANVIL_SOCK`, else `$XDG_RUNTIME_DIR/anvil.sock`).
        #[arg(long)]
        sock: Option<PathBuf>,
        /// State directory (`ANVIL_ROOT`, else `~/.anvil`).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Maintain the agent catalog (`agents.json`).
    Agent(anvil::catalog_cmd::AgentCli),
}

const LONG_ABOUT: &str = "\
The daemon holds every process on the box. A shell sits on a PTY. An \
ACP agent sits on stdio. The client tiles their screens and draws a \
roster of what they are doing.

With no command, this binary is the client: it attaches to the daemon. \
If none is listening, it starts this binary. It will not attach to
another build. Detach never kills a process.

Prefix is Ctrl-B. Prefix q detaches. The socket is \
$XDG_RUNTIME_DIR/anvil.sock (ANVIL_SOCK). State is ~/.anvil (ANVIL_ROOT).";

const AFTER_HELP: &str = "\
Catalog:
  anvil agent list | show NAME | default [NAME]
  anvil agent add NAME --program P [--acp-only] [--acp-program P] [--adopt A] [--http] [--resume 'F']
  anvil agent from SHIPPED [--as NAME]
  anvil agent rm NAME
  anvil agent seed [--force]

Channel (PATH wrapper scripts/launch, not this binary):
  anvil channel show
  anvil channel stable
  anvil channel dev <worktree>";

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Daemon { sock, root }) => {
            let sock = sock.unwrap_or_else(daemon::default_sock);
            let root = root
                .or(cli.root)
                .unwrap_or_else(|| std::env::var("ANVIL_ROOT").unwrap_or_else(|_| default_root()).into());
            daemon::run(root, sock)
        }
        Some(Command::Agent(cmd)) => {
            let mut cmd = cmd;
            if cmd.root.is_none() {
                cmd.root = cli.root;
            }
            anvil::catalog_cmd::run(cmd)
        }
        None => {
            let sock = daemon::default_sock();
            let root = cli
                .root
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| std::env::var("ANVIL_ROOT").unwrap_or_else(|_| default_root()));
            if cli.restart {
                daemon::restart(&sock, std::path::Path::new(&root))?;
            } else {
                daemon::ensure_running(&sock, std::path::Path::new(&root))?;
            }
            anvil::tui::run(&sock)
        }
    }
}

fn default_root() -> String {
    std::env::var("HOME").map(|h| format!("{h}/.anvil")).unwrap_or_else(|_| ".anvil".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn long_help() -> String {
        let mut buf = Vec::new();
        Cli::command()
            .write_long_help(&mut buf)
            .expect("help writes");
        String::from_utf8(buf).expect("help is utf-8")
    }

    #[test]
    fn help_names_the_client_the_daemon_and_channel() {
        let help = long_help();
        assert!(help.contains("ACP"), "{help}");
        assert!(help.contains("client"), "{help}");
        assert!(help.contains("daemon"), "{help}");
        assert!(help.contains("--restart"), "{help}");
        assert!(help.contains("channel"), "{help}");
        assert!(help.contains("agent"), "{help}");
        assert!(help.contains("Ctrl-B"), "{help}");
        assert!(!help.contains("Terminal multiplexer"), "{help}");
    }

    #[test]
    fn daemon_help_says_it_is_the_parent() {
        let mut buf = Vec::new();
        Cli::command()
            .find_subcommand_mut("daemon")
            .expect("daemon")
            .write_long_help(&mut buf)
            .expect("help writes");
        let help = String::from_utf8(buf).expect("help is utf-8");
        assert!(help.contains("Parent of every process"), "{help}");
        assert!(help.contains("ANVIL_SOCK"), "{help}");
        assert!(help.contains("ANVIL_ROOT"), "{help}");
    }
}
