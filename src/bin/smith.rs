//! smith — the TUI. Stands at the anvil.

use std::io;
use std::path::PathBuf;

use anvil::default_hammer;
use anvil::tui::{self, Launch};
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "smith",
    about = "TUI: type a prompt, the model writes Python, the hammer strikes"
)]
struct Cli {
    /// Raw store (no rail). Default is the frame under --root.
    #[arg(long, env = "ANVIL_STORE")]
    store: Option<PathBuf>,
    #[arg(long, env = "ANVIL_ROOT")]
    root: Option<PathBuf>,
    #[arg(long, short = 's')]
    session: Option<String>,
    #[arg(long)]
    workspace: Option<String>,
    #[arg(long)]
    catalog: Option<String>,
    #[arg(long, env = "ANVIL_HAMMER")]
    hammer: Option<PathBuf>,
    #[arg(long, env = "ANVIL_CONFIG")]
    config: Option<PathBuf>,
    #[arg(long, short = 'p')]
    provider: Option<String>,
    #[arg(long, short = 'm')]
    model: Option<String>,
    #[arg(long)]
    cwd: Option<PathBuf>,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    tui::run(Launch {
        store: cli.store,
        root: cli.root,
        session: cli.session,
        workspace: cli.workspace,
        catalog: cli.catalog,
        hammer: cli.hammer.unwrap_or_else(default_hammer),
        config: cli.config,
        provider: cli.provider,
        model: cli.model,
        cwd: cli
            .cwd
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
    })
}
