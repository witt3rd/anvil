use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use anvil::catalog;
use anvil::complete;
use anvil::config::{Auth, Config};
use anvil::oauth;
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

    /// Config file. Default: $ANVIL_CONFIG or ~/.config/anvil/config.yaml
    #[arg(long, env = "ANVIL_CONFIG", global = true)]
    config: Option<PathBuf>,

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
    /// List named providers from config.yaml. Never prints secrets.
    Providers,
    /// Run a vendor login (oauth providers only). Exemplar: `grok login`.
    Login { name: String },
    /// List models. Uses the 24h cache unless --refresh.
    Models {
        name: Option<String>,
        /// Force a network refresh.
        #[arg(long)]
        refresh: bool,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// One-shot chat completion (smoke the HTTP path).
    Complete {
        prompt: Vec<String>,
        #[arg(long, short = 'p')]
        provider: Option<String>,
        #[arg(long, short = 'm')]
        model: Option<String>,
    },
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
        Command::Providers => providers(cli.config.as_deref()),
        Command::Login { name } => login(cli.config.as_deref(), &name),
        Command::Models {
            name,
            refresh,
            json,
        } => models(cli.config.as_deref(), name.as_deref(), refresh, json),
        Command::Complete {
            prompt,
            provider,
            model,
        } => complete_cmd(
            cli.config.as_deref(),
            provider.as_deref(),
            model.as_deref(),
            &prompt,
        ),
    }
}

fn load_cfg(path: Option<&std::path::Path>) -> Result<(PathBuf, Config), String> {
    match path {
        Some(p) => Config::load_from(p),
        None => Config::load(),
    }
    .map_err(|e| e.to_string())
}

fn providers(path: Option<&std::path::Path>) -> ExitCode {
    let (cfg_path, cfg) = match load_cfg(path) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    println!("config\t{}", cfg_path.display());
    if let Some(name) = cfg.default_provider.as_deref() {
        println!("default_provider\t{name}");
    }
    if let Some(model) = cfg.default_model.as_deref() {
        println!("default_model\t{model}");
    }
    for (name, provider) in &cfg.providers {
        let mark = if cfg.default_provider.as_deref() == Some(name.as_str()) {
            "*"
        } else {
            " "
        };
        let (kind, ready) = match &provider.auth {
            Auth::ApiKey { key } => {
                let ready = anvil::secret::resolve(key).is_ok();
                ("api_key", ready)
            }
            Auth::Oauth { vendor } => ("oauth", oauth::has_login(*vendor)),
        };
        let base = provider.base_url().unwrap_or_else(|| "-".into());
        let model = provider.default_model.as_deref().unwrap_or("-");
        println!("{mark}{name}\t{kind}\tready={ready}\t{base}\tmodel={model}");
    }
    ExitCode::SUCCESS
}

fn login(path: Option<&std::path::Path>, name: &str) -> ExitCode {
    let (_cfg_path, cfg) = match load_cfg(path) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    let (_, provider) = match cfg.provider(Some(name)) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    match &provider.auth {
        Auth::ApiKey { .. } => {
            eprintln!("anvil: provider '{name}' uses api_key, not oauth. Nothing to log in.");
            ExitCode::from(2)
        }
        Auth::Oauth { vendor } => match oauth::login(*vendor) {
            Ok(()) => {
                if oauth::has_login(*vendor) {
                    println!("logged in ({name})");
                    ExitCode::SUCCESS
                } else {
                    fail("login command succeeded but no cached credential was found")
                }
            }
            Err(err) => fail(err),
        },
    }
}

fn models(
    path: Option<&std::path::Path>,
    name: Option<&str>,
    refresh: bool,
    json: bool,
) -> ExitCode {
    let (_cfg_path, cfg) = match load_cfg(path) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    let (name, provider) = match cfg.provider(name) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    match catalog::models(name, provider, refresh) {
        Ok(cache) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&cache).unwrap());
            } else {
                println!(
                    "provider\t{}\nurl\t{}\ncount\t{}",
                    cache.provider,
                    cache.url,
                    cache.models.len()
                );
                for model in &cache.models {
                    match &model.owned_by {
                        Some(owner) => println!("{}\t{owner}", model.id),
                        None => println!("{}", model.id),
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => fail(err),
    }
}

fn complete_cmd(
    path: Option<&std::path::Path>,
    provider: Option<&str>,
    model: Option<&str>,
    prompt: &[String],
) -> ExitCode {
    let (_cfg_path, cfg) = match load_cfg(path) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    let (_name, prov) = match cfg.provider(provider) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    let Some(model) = cfg.model_for(prov, model) else {
        return fail("no model: set default_model or pass --model");
    };
    let prompt = prompt.join(" ");
    if prompt.trim().is_empty() {
        return fail("complete needs a prompt");
    }
    match complete::complete(prov, &model, &prompt) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => fail(err),
    }
}

fn fail(err: impl std::fmt::Display) -> ExitCode {
    eprintln!("anvil: {err}");
    ExitCode::from(1)
}
