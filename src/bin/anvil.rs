use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use anvil::{default_hammer, default_store, Anvil};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "anvil", about = "Harness: spawn a hammer and issue strikes")]
struct Cli {
    /// Store directory (namespace.pkl). Default: $ANVIL_STORE or ~/.anvil/default
    #[arg(long, env = "ANVIL_STORE", global = true)]
    store: Option<PathBuf>,

    /// Path to hammer/hammer.py
    #[arg(long, env = "ANVIL_HAMMER", global = true)]
    hammer: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run one strike and print the reply as JSON. Code from args or stdin.
    Strike { code: Vec<String> },
    /// Reset the store namespace.
    Reset,
    /// Reserved: smith talks to the harness in-process in v0.
    Serve,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let store = cli.store.unwrap_or_else(default_store);
    let hammer = cli.hammer.unwrap_or_else(default_hammer);

    match cli.cmd {
        Command::Serve => {
            eprintln!("anvil serve is not implemented. Run `smith` — it owns the hammer.");
            ExitCode::from(2)
        }
        Command::Reset => match Anvil::open(&store, &hammer) {
            Ok(mut a) => match a.reset() {
                Ok(reply) => {
                    println!("{}", serde_json::to_string(&reply).unwrap());
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            },
            Err(err) => fail(err),
        },
        Command::Strike { code } => {
            let source = if code.is_empty() {
                let mut buf = String::new();
                if let Err(err) = io::stdin().read_to_string(&mut buf) {
                    return fail(err);
                }
                buf
            } else {
                code.join(" ")
            };
            match Anvil::open(&store, &hammer) {
                Ok(mut a) => match a.strike(&source) {
                    Ok(reply) => {
                        println!("{}", serde_json::to_string_pretty(&reply).unwrap());
                        if reply.ok {
                            ExitCode::SUCCESS
                        } else {
                            ExitCode::from(1)
                        }
                    }
                    Err(err) => fail(err),
                },
                Err(err) => fail(err),
            }
        }
    }
}

fn fail(err: impl std::fmt::Display) -> ExitCode {
    eprintln!("anvil: {err}");
    ExitCode::from(1)
}
