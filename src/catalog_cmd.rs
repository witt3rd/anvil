//! `anvil agent` — maintain the catalog without hand-editing JSON.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::catalog::{Agent, Agents, Door, InhibitDoor};

#[derive(Parser)]
pub struct AgentCli {
    /// State directory (`ANVIL_ROOT`, else `~/.anvil`).
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    #[command(subcommand)]
    pub cmd: AgentCmd,
}

#[derive(Subcommand)]
pub enum AgentCmd {
    /// List catalog rows.
    List,
    /// Print one row as JSON.
    Show {
        name: String,
    },
    /// Show or set the default agent.
    Default {
        name: Option<String>,
    },
    /// Add or replace a row.
    Add {
        name: String,
        #[arg(long)]
        program: String,
        #[arg(long)]
        acp_program: Option<String>,
        #[arg(long)]
        acp_only: bool,
        /// Basename that means this row in a shell (repeatable).
        #[arg(long)]
        adopt: Vec<String>,
        /// Native HTTP door; paths come from the shipped catalog.
        #[arg(long)]
        http: bool,
        /// Cmdline needles for an inhibit door (repeatable).
        #[arg(long)]
        inhibit: Vec<String>,
        /// Extra argv to reopen a pane's inner session. `{session}` is the id.
        #[arg(long)]
        resume: Option<String>,
        /// Replace a row that already exists.
        #[arg(long)]
        replace: bool,
    },
    /// Copy a shipped (or existing) row, optionally under a new name.
    From {
        name: String,
        #[arg(long)]
        r#as: Option<String>,
    },
    /// Remove a row.
    Rm {
        name: String,
    },
    /// Write the shipped catalog. Refuses to overwrite unless `--force`.
    Seed {
        #[arg(long)]
        force: bool,
    },
}

pub fn run(cli: AgentCli) -> io::Result<()> {
    let root = cli.root.unwrap_or_else(crate::catalog::default_root);
    match cli.cmd {
        AgentCmd::List => list(&root),
        AgentCmd::Show { name } => show(&root, &name),
        AgentCmd::Default { name } => set_default(&root, name.as_deref()),
        AgentCmd::Add {
            name,
            program,
            acp_program,
            acp_only,
            adopt,
            http,
            inhibit,
            resume,
            replace,
        } => add(
            &root,
            Agent {
                name,
                program,
                acp_program,
                acp_only,
                adopt,
                door: door_from_flags(http, inhibit),
                resume,
                ..Default::default()
            },
            replace,
        ),
        AgentCmd::From { name, r#as } => from_row(&root, &name, r#as.as_deref()),
        AgentCmd::Rm { name } => rm(&root, &name),
        AgentCmd::Seed { force } => seed(&root, force),
    }
}

fn door_from_flags(http: bool, inhibit: Vec<String>) -> Door {
    if http {
        Door::Http(Agents::shipped_http())
    } else if !inhibit.is_empty() {
        Door::Inhibit(InhibitDoor {
            contains: inhibit,
            files: None,
        })
    } else {
        Door::None
    }
}

fn list(root: &std::path::Path) -> io::Result<()> {
    let agents = Agents::load_from(root)?;
    for a in &agents.agents {
        let mark = if a.name == agents.default { "*" } else { " " };
        let seats: Vec<_> = a.seats().iter().map(|s| s.label()).collect();
        println!("{mark} {:<12} {}  ({})", a.name, a.program, seats.join(", "));
    }
    Ok(())
}

fn show(root: &std::path::Path, name: &str) -> io::Result<()> {
    let agents = Agents::load_from(root)?;
    let agent = agents
        .by_name(name)
        .ok_or_else(|| io::Error::other(format!("no agent named {name}")))?;
    let mut out = serde_json::to_string_pretty(agent).map_err(io::Error::other)?;
    out.push('\n');
    io::stdout().write_all(out.as_bytes())?;
    Ok(())
}

fn set_default(root: &std::path::Path, name: Option<&str>) -> io::Result<()> {
    let mut agents = Agents::load_from(root)?;
    match name {
        None => {
            println!("{}", agents.default);
            Ok(())
        }
        Some(name) => {
            if agents.by_name(name).is_none() {
                return Err(io::Error::other(format!("no agent named {name}")));
            }
            agents.default = name.to_string();
            agents.save(root)?;
            println!("default={name}");
            Ok(())
        }
    }
}

fn add(root: &std::path::Path, agent: Agent, replace: bool) -> io::Result<()> {
    let mut agents = Agents::load_from(root)?;
    if agents.by_name(&agent.name).is_some() && !replace {
        return Err(io::Error::other(format!(
            "{} already exists (pass --replace)",
            agent.name
        )));
    }
    let name = agent.name.clone();
    agents.upsert(agent);
    agents.save(root)?;
    println!("wrote {name}");
    Ok(())
}

fn from_row(root: &std::path::Path, name: &str, as_name: Option<&str>) -> io::Result<()> {
    let shipped = Agents::default();
    let mut agents = Agents::load_from(root)?;
    let mut row = shipped
        .by_name(name)
        .or_else(|| agents.by_name(name))
        .cloned()
        .ok_or_else(|| io::Error::other(format!("no shipped or local row named {name}")))?;
    if let Some(as_name) = as_name {
        row.name = as_name.to_string();
        if row.adopt.is_empty() {
            row.adopt = vec![as_name.to_string()];
        }
    }
    let out = row.name.clone();
    agents.upsert(row);
    agents.save(root)?;
    println!("wrote {out}");
    Ok(())
}

fn rm(root: &std::path::Path, name: &str) -> io::Result<()> {
    let mut agents = Agents::load_from(root)?;
    if !agents.remove(name) {
        return Err(io::Error::other(format!("no agent named {name}")));
    }
    agents.save(root)?;
    println!("removed {name}");
    Ok(())
}

fn seed(root: &std::path::Path, force: bool) -> io::Result<()> {
    let path = root.join("agents.json");
    if path.is_file() && !force {
        return Err(io::Error::other(
            "agents.json already exists (pass --force to replace it with the shipped catalog)",
        ));
    }
    Agents::default().save(root)?;
    println!("wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_list_rm_round_trip() {
        let dir = std::env::temp_dir().join(format!("anvil-agent-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        seed(&dir, true).unwrap();
        add(
            &dir,
            Agent {
                name: "pi".into(),
                program: "pi".into(),
                acp_only: true,
                ..Default::default()
            },
            false,
        )
        .unwrap();
        let agents = Agents::load_from(&dir).unwrap();
        assert!(agents.by_name("pi").unwrap().acp_only);
        rm(&dir, "pi").unwrap();
        assert!(Agents::load_from(&dir).unwrap().by_name("pi").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
