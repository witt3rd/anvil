//! smith — the TUI. Stands at the anvil.

use std::io;
use std::path::PathBuf;

use anvil::tui::{self, Launch};
use anvil::{default_hammer, default_store};
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "smith",
    about = "TUI: type a prompt, the model writes Python, the hammer strikes"
)]
struct Cli {
    #[arg(long, env = "ANVIL_STORE")]
    store: Option<PathBuf>,
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
        store: cli.store.unwrap_or_else(default_store),
        hammer: cli.hammer.unwrap_or_else(default_hammer),
        config: cli.config,
        provider: cli.provider,
        model: cli.model,
        cwd: cli
            .cwd
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
    })
}
