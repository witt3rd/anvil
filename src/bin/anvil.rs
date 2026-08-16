use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use anvil::ask::{self, HttpCompleter};
use anvil::catalog;
use anvil::complete;
use anvil::config::{Auth, Config};
use anvil::frame::{self, FrameRoot, MemberRef};
use anvil::oauth;
use anvil::{default_hammer, Anvil};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "anvil", about = "Harness: spawn a hammer and issue strikes")]
struct Cli {
    /// Raw store directory (namespace.pkl). Conflicts with --session.
    #[arg(long, env = "ANVIL_STORE", global = true)]
    store: Option<PathBuf>,

    /// Frame root. Default: $ANVIL_ROOT or ~/.anvil
    #[arg(long, env = "ANVIL_ROOT", global = true)]
    root: Option<PathBuf>,

    /// Named session under the frame root.
    #[arg(long, global = true)]
    session: Option<String>,

    /// Run this command on HOST over SSH. Sessions stay there.
    #[arg(long, short = 'R', global = true)]
    #[allow(dead_code)]
    remote: Option<String>,

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
    /// Own hammers on a unix socket. smith attaches; close the casing, work stays.
    Serve {
        #[arg(long, env = "ANVIL_SOCK")]
        sock: Option<PathBuf>,
        /// Stop a running serve.
        #[arg(long)]
        stop: bool,
        /// Print whether serve is up.
        #[arg(long)]
        status: bool,
        /// Install and enable the user systemd unit (cold after reboot).
        #[arg(long)]
        install: bool,
        /// Disable and remove the user systemd unit.
        #[arg(long)]
        uninstall: bool,
    },
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
    /// One-shot chat completion (smoke the HTTP path). Does not strike.
    Complete {
        prompt: Vec<String>,
        #[arg(long, short = 'p')]
        provider: Option<String>,
        #[arg(long, short = 'm')]
        model: Option<String>,
    },
    /// Model writes Python; hammer runs it; print the strike result.
    Ask {
        prompt: Vec<String>,
        #[arg(long, short = 'p')]
        provider: Option<String>,
        #[arg(long, short = 'm')]
        model: Option<String>,
    },
    /// Named sessions (agentic processes) on disk.
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// Named benches: collections of members. Destroying one keeps the members.
    Workspace {
        #[command(subcommand)]
        cmd: WorkspaceCmd,
    },
    /// Named intents: collections of workspaces. Destroying one keeps the workspaces.
    Catalog {
        #[command(subcommand)]
        cmd: CatalogCmd,
    },
    /// Live fibers, services, slots (serve if up; else disk-cold).
    Inspect {
        #[arg(long)]
        json: bool,
    },
    /// Dump the live prof ring (serve if up; else this process).
    Prof {
        #[arg(long)]
        json: bool,
        /// Last N samples (default 24).
        #[arg(long, default_value_t = 24)]
        last: usize,
    },
    /// Mount a temporary fiber (in memory). First toy: `clock`. Needs serve.
    Mount {
        kind: String,
        #[arg(long)]
        slot: Option<String>,
    },
    /// Unmount a temporary fiber by id (`dyn-1`). Needs serve.
    Unmount { id: String },
    /// Login-shell members (PTY). Serve owns the process.
    Pty {
        #[command(subcommand)]
        cmd: PtyCmd,
    },
    /// Scratch text editors. Serve owns the buffer; file is edits/<id>.txt.
    Edit {
        #[command(subcommand)]
        cmd: EditCmd,
    },
}

#[derive(Subcommand)]
enum SessionCmd {
    List,
    New {
        /// Omit to mint a short unused word.
        name: Option<String>,
    },
    Show {
        name: String,
    },
    /// Print the event log (source of truth).
    Log {
        name: String,
        #[arg(long)]
        json: bool,
    },
    Rm {
        name: String,
    },
}

#[derive(Subcommand)]
enum WorkspaceCmd {
    List,
    New {
        name: String,
    },
    Add {
        workspace: String,
        session: String,
        /// Add a login-shell member instead of an anvil session.
        #[arg(long)]
        pty: bool,
        /// Add a clock member (remounted on attach).
        #[arg(long)]
        clock: bool,
        /// Add a diagnose pane that projects this session's event log.
        #[arg(long)]
        log: bool,
        /// Add a scratch text editor (edits/<name>.txt).
        #[arg(long)]
        edit: bool,
    },
    Rm {
        name: String,
    },
}

#[derive(Subcommand)]
enum PtyCmd {
    /// Name a bash member and add it to a workspace. Does not create a session.
    New {
        name: String,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// List PTY members (hot if serve is up).
    List,
    /// Print the live screen. Needs serve. Warms $SHELL if cold.
    Snap { name: String },
    /// Type into the PTY. Appends Enter unless --raw. Needs serve.
    Write {
        name: String,
        text: Vec<String>,
        /// Send bytes as-is (no trailing CR).
        #[arg(long)]
        raw: bool,
    },
}

#[derive(Subcommand)]
enum EditCmd {
    New {
        name: String,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Print the buffer. Serve if up, else the file on disk.
    Snap { name: String },
    /// Replace the buffer (or stdin). Needs serve to keep cursor; else writes the file.
    Write { name: String, text: Vec<String> },
}

#[derive(Subcommand)]
enum CatalogCmd {
    List,
    New { name: String },
    Add { catalog: String, workspace: String },
    Rm { name: String },
}

fn main() -> ExitCode {
    let (remote, rest) = anvil::remote::strip_remote(std::env::args().skip(1));
    if let Some(host) = remote {
        return ExitCode::from(anvil::remote::exec(&host, "anvil", &rest, false) as u8);
    }
    let cli = Cli::parse();
    anvil::prof::init();
    let hammer = cli.hammer.clone().unwrap_or_else(default_hammer);

    match &cli.cmd {
        Command::Session { cmd } => return session_cmd(cli.root.as_deref(), cmd),
        Command::Workspace { cmd } => return workspace_cmd(cli.root.as_deref(), cmd),
        Command::Catalog { cmd } => return catalog_cmd(cli.root.as_deref(), cmd),
        Command::Serve {
            sock,
            stop,
            status,
            install,
            uninstall,
        } => {
            return serve_cmd(&cli, sock.clone(), *stop, *status, *install, *uninstall);
        }
        Command::Inspect { json } => return inspect_cmd(cli.root.as_deref(), *json),
        Command::Prof { json, last } => return prof_cmd(*json, *last),
        Command::Mount { kind, slot } => return mount_cmd(kind, slot.as_deref()),
        Command::Unmount { id } => return unmount_cmd(id),
        Command::Pty { cmd } => return pty_cmd(cli.root.as_deref(), cmd),
        Command::Edit { cmd } => return edit_cmd(cli.root.as_deref(), cmd),
        _ => {}
    }

    let store = match resolve_store(&cli) {
        Ok(p) => p,
        Err(err) => return fail(err),
    };

    match cli.cmd {
        Command::Reset => {
            match serve_or_local(&cli, &store, &hammer, |c, s| c.reset(s), |a| a.reset()) {
                Ok(reply) => {
                    println!("{}", serde_json::to_string(&reply).unwrap());
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            }
        }
        Command::Strike { ref code } => {
            let source = if code.is_empty() {
                let mut buf = String::new();
                if let Err(err) = io::stdin().read_to_string(&mut buf) {
                    return fail(err);
                }
                buf
            } else {
                code.join(" ")
            };
            match serve_or_local(
                &cli,
                &store,
                &hammer,
                |c, s| c.strike(s, &source),
                |a| a.strike(&source),
            ) {
                Ok(reply) => {
                    println!("{}", serde_json::to_string_pretty(&reply).unwrap());
                    if reply.ok {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(1)
                    }
                }
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
        Command::Ask {
            ref prompt,
            ref provider,
            ref model,
        } => ask_cmd(
            &cli,
            &store,
            &hammer,
            provider.as_deref(),
            model.as_deref(),
            prompt,
        ),
        Command::Session { .. }
        | Command::Workspace { .. }
        | Command::Catalog { .. }
        | Command::Serve { .. }
        | Command::Inspect { .. }
        | Command::Mount { .. }
        | Command::Unmount { .. }
        | Command::Pty { .. }
        | Command::Edit { .. }
        | Command::Prof { .. } => unreachable!("handled above"),
    }
}

fn serve_cmd(
    cli: &Cli,
    sock: Option<PathBuf>,
    stop: bool,
    status: bool,
    install: bool,
    uninstall: bool,
) -> ExitCode {
    let sock = sock.unwrap_or_else(anvil::serve::default_sock);
    if uninstall {
        return match anvil::serve::unit::uninstall() {
            Ok(()) => {
                println!("uninstalled\t{}", anvil::serve::UNIT_NAME);
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        };
    }
    if install {
        let _ = anvil::serve::stop(&sock);
        return match anvil::serve::unit::install() {
            Ok(path) => {
                println!("installed\t{}", path.display());
                println!(
                    "unit\t{} enabled={} active={} linger={}",
                    anvil::serve::UNIT_NAME,
                    anvil::serve::unit::enabled(),
                    anvil::serve::unit::active(),
                    anvil::serve::unit::linger_on()
                );
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        };
    }
    if status {
        println!("linger\t{}", anvil::serve::unit::linger_on());
        if anvil::serve::unit::enabled() {
            println!(
                "unit\t{} enabled={} active={}",
                anvil::serve::UNIT_NAME,
                true,
                anvil::serve::unit::active()
            );
        }
        return match anvil::serve::status(&sock) {
            Ok(true) => {
                println!("up\t{}", sock.display());
                ExitCode::SUCCESS
            }
            Ok(false) => {
                println!("down\t{}", sock.display());
                ExitCode::from(1)
            }
            Err(err) => fail(err),
        };
    }
    if stop {
        // Socket only. Do not call `systemctl stop` from here: the unit's
        // ExecStop is this same command, and that pair deadlocks until
        // TimeoutStopSec. systemd sees the main PID exit after Shutdown.
        return match anvil::serve::stop(&sock) {
            Ok(()) => {
                println!("stopped\t{}", sock.display());
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        };
    }
    let root = cli.root.clone().unwrap_or_else(frame::default_root);
    let hammer = cli.hammer.clone().unwrap_or_else(default_hammer);
    match anvil::serve::run(anvil::serve::ServeOpts {
        root,
        hammer,
        config: cli.config.clone(),
        sock,
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => fail(err),
    }
}

fn session_name(cli: &Cli) -> String {
    cli.session.clone().unwrap_or_else(|| "default".into())
}

fn serve_or_local(
    cli: &Cli,
    store: &std::path::Path,
    hammer: &std::path::Path,
    via: impl FnOnce(&mut anvil::serve::Client, &str) -> io::Result<anvil::StrikeReply>,
    local: impl FnOnce(&mut Anvil) -> Result<anvil::StrikeReply, anvil::AnvilError>,
) -> Result<anvil::StrikeReply, String> {
    if cli.store.is_none() {
        if let Ok(mut c) = anvil::serve::Client::connect(anvil::serve::default_sock()) {
            return via(&mut c, &session_name(cli)).map_err(|e| e.to_string());
        }
    }
    let mut a = Anvil::open(store, hammer).map_err(|e| e.to_string())?;
    local(&mut a).map_err(|e| e.to_string())
}

fn open_root(root: Option<&std::path::Path>) -> Result<FrameRoot, String> {
    FrameRoot::open(root.map(PathBuf::from).unwrap_or_else(frame::default_root))
        .map_err(|e| e.to_string())
}

fn resolve_store(cli: &Cli) -> Result<PathBuf, String> {
    if cli.store.is_some() && cli.session.is_some() {
        return Err("use --store or --session, not both".into());
    }
    if let Some(store) = &cli.store {
        return Ok(store.clone());
    }
    let root = open_root(cli.root.as_deref())?;
    let id = cli.session.as_deref().unwrap_or("default");
    if !root.session_exists(id) {
        if id == "default" {
            root.ensure_defaults().map_err(|e| e.to_string())?;
        } else {
            root.create_session(id).map_err(|e| e.to_string())?;
        }
    }
    Ok(root.session_dir(id))
}

fn session_cmd(root: Option<&std::path::Path>, cmd: &SessionCmd) -> ExitCode {
    let root = match open_root(root) {
        Ok(r) => r,
        Err(err) => return fail(err),
    };
    match cmd {
        SessionCmd::List => match root.list_sessions() {
            Ok(list) => {
                for s in list {
                    println!("{}\t{}", s.id, s.dir.display());
                }
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        },
        SessionCmd::New { name } => {
            let minted = match name.as_deref() {
                Some(n) if !n.trim().is_empty() => n.to_string(),
                _ => match root.mint_name() {
                    Ok(n) => n,
                    Err(err) => return fail(err),
                },
            };
            match root.create_session(&minted) {
                Ok(s) => {
                    println!("{}\t{}", s.id, s.dir.display());
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            }
        }
        SessionCmd::Show { name } => match root.session(name) {
            Ok(s) => {
                println!("id\t{}", s.id);
                println!("dir\t{}", s.dir.display());
                println!("created\t{}", s.meta.created);
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        },
        SessionCmd::Log { name, json } => match root.load_events(name) {
            Ok(events) => {
                if *json {
                    println!("{}", serde_json::to_string_pretty(&events).unwrap());
                } else {
                    for ev in events {
                        let vis = if ev.body.model_visible() { "v" } else { " " };
                        println!(
                            "{:>4} {vis} {}\t{}",
                            ev.seq,
                            ev.ts,
                            serde_json::to_string(&ev.body).unwrap()
                        );
                    }
                }
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        },
        SessionCmd::Rm { name } => match root.delete_session(name) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => fail(err),
        },
    }
}

fn inspect_cmd(root: Option<&std::path::Path>, json: bool) -> ExitCode {
    let report = match anvil::serve::Client::connect(anvil::serve::default_sock()) {
        Ok(mut c) => match c.inspect() {
            Ok(r) => r,
            Err(err) => return fail(err),
        },
        Err(_) => match cold_inspect(root) {
            Ok(r) => r,
            Err(err) => return fail(err),
        },
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return ExitCode::SUCCESS;
    }
    println!("root\t{}", report.root);
    println!("sock\t{}", report.sock);
    println!("services");
    for s in &report.services {
        println!("  {}\t{}\t{}\tevents={}", s.name, s.kind, s.state, s.events);
    }
    println!("fibers");
    for f in &report.fibers {
        println!("  {}\t{}\t{}", f.name, f.kind, f.state);
    }
    println!("slots");
    for sl in &report.slots {
        let who = sl.occupant.as_deref().unwrap_or("-");
        match &sl.text {
            Some(text) => println!("  {}\t{}\t{}\t{}", sl.name, sl.kind, who, text),
            None => println!("  {}\t{}\t{}", sl.name, sl.kind, who),
        }
    }
    if !report.workspaces.is_empty() {
        println!("workspaces\t{}", report.workspaces.join(","));
    }
    if !report.catalogs.is_empty() {
        println!("catalogs\t{}", report.catalogs.join(","));
    }
    print_prof(&report.prof, 12);
    ExitCode::SUCCESS
}

fn prof_cmd(json: bool, last: usize) -> ExitCode {
    let snap = match anvil::serve::Client::connect(anvil::serve::default_sock()) {
        Ok(mut c) => match c.inspect() {
            Ok(r) => r.prof,
            Err(_) => anvil::prof::snapshot(),
        },
        Err(_) => anvil::prof::snapshot(),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&snap).unwrap());
        return ExitCode::SUCCESS;
    }
    print_prof(&snap, last);
    ExitCode::SUCCESS
}

fn print_prof(snap: &anvil::prof::Snap, last: usize) {
    if let Some(t) = &snap.last_model {
        let compact = t.compact();
        if !compact.is_empty() {
            println!("model\t{compact}");
        }
    }
    if !snap.counters.is_empty() {
        println!("counters");
        for (k, v) in &snap.counters {
            println!("  {k}\t{v}");
        }
    }
    let samples = if snap.samples.len() > last {
        &snap.samples[snap.samples.len() - last..]
    } else {
        &snap.samples[..]
    };
    if samples.is_empty() {
        println!("prof\t(empty — serve up? last ask?)");
        return;
    }
    println!("prof");
    for s in samples {
        let extra = s.extra.as_deref().unwrap_or("");
        let toks = s.tokens.map(|n| format!("\t{n} tok")).unwrap_or_default();
        println!(
            "  {}\t{}\t{}{}{}",
            s.group,
            s.name,
            anvil::prof::fmt_ns(s.dur_ns),
            toks,
            if extra.is_empty() {
                String::new()
            } else {
                format!("\t{extra}")
            }
        );
    }
}

fn mount_cmd(kind: &str, slot: Option<&str>) -> ExitCode {
    let mut c = match anvil::serve::Client::connect(anvil::serve::default_sock()) {
        Ok(c) => c,
        Err(_) => return fail("serve is down — start smith or `anvil serve`"),
    };
    match c.mount(kind, slot) {
        Ok((id, seat)) => {
            println!("{id}\t{kind}\t{seat}");
            ExitCode::SUCCESS
        }
        Err(err) => fail(err),
    }
}

fn unmount_cmd(id: &str) -> ExitCode {
    let mut c = match anvil::serve::Client::connect(anvil::serve::default_sock()) {
        Ok(c) => c,
        Err(_) => return fail("serve is down — nothing to unmount"),
    };
    match c.unmount(id) {
        Ok(()) => {
            println!("unmounted\t{id}");
            ExitCode::SUCCESS
        }
        Err(err) => fail(err),
    }
}

fn cold_inspect(root: Option<&std::path::Path>) -> Result<anvil::serve::Report, String> {
    let root = open_root(root)?;
    let _ = root.ensure_defaults();
    let sessions = root.list_sessions().map_err(|e| e.to_string())?;
    let mut services = Vec::new();
    let mut fibers = Vec::new();
    for sess in &sessions {
        let events = root
            .load_events(&sess.id)
            .map(|e| e.len() as u64)
            .unwrap_or(0);
        services.push(anvil::serve::Service {
            name: sess.id.clone(),
            kind: "session".into(),
            state: "cold".into(),
            events,
        });
        fibers.push(anvil::serve::Fiber {
            name: format!("adapter/{}", sess.id),
            kind: "adapter".into(),
            state: "pending".into(),
        });
    }
    let mut seen_pty = Vec::new();
    if let Ok(workspaces) = root.list_workspaces() {
        for ws in workspaces {
            for member in ws.members {
                if !member.is_pty() {
                    continue;
                }
                let name = member.id().to_string();
                if seen_pty.iter().any(|n| n == &name) {
                    continue;
                }
                seen_pty.push(name.clone());
                services.push(anvil::serve::Service {
                    name: name.clone(),
                    kind: "pty".into(),
                    state: "cold".into(),
                    events: 0,
                });
                fibers.push(anvil::serve::Fiber {
                    name: format!("adapter/{name}"),
                    kind: "pty".into(),
                    state: "pending".into(),
                });
            }
        }
    }
    let front = root.layout("default").ok().and_then(|l| l.front_session);
    Ok(anvil::serve::Report {
        root: root.root().display().to_string(),
        sock: "(down)".into(),
        services,
        fibers,
        slots: vec![
            anvil::serve::Slot {
                name: "casing.rail".into(),
                kind: "chrome".into(),
                occupant: None,
                text: None,
            },
            anvil::serve::Slot {
                name: "casing.main".into(),
                kind: "stage".into(),
                occupant: front.clone(),
                text: None,
            },
            anvil::serve::Slot {
                name: "casing.status".into(),
                kind: "chrome".into(),
                occupant: None,
                text: None,
            },
            anvil::serve::Slot {
                name: "session.transcript".into(),
                kind: "smith".into(),
                occupant: front,
                text: None,
            },
        ],
        workspaces: root
            .list_workspaces()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|w| w.name)
            .collect(),
        catalogs: root
            .list_catalogs()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|c| c.name)
            .collect(),
        prof: anvil::prof::snapshot(),
    })
}

fn workspace_cmd(root: Option<&std::path::Path>, cmd: &WorkspaceCmd) -> ExitCode {
    let root = match open_root(root) {
        Ok(r) => r,
        Err(err) => return fail(err),
    };
    match cmd {
        WorkspaceCmd::List => match root.list_workspaces() {
            Ok(list) => {
                for ws in list {
                    let members = ws
                        .members
                        .iter()
                        .map(|m| m.label())
                        .collect::<Vec<_>>()
                        .join(",");
                    println!("{}\t{}", ws.name, members);
                }
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        },
        WorkspaceCmd::New { name } => match root.create_workspace(name) {
            Ok(ws) => {
                println!("{}", ws.name);
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        },
        WorkspaceCmd::Add {
            workspace,
            session,
            pty,
            clock,
            log,
            edit,
        } => {
            if !pty && !clock && !edit && !root.session_exists(session) {
                if let Err(err) = root.create_session(session) {
                    return fail(err);
                }
            }
            let mut ws = match root.workspace(workspace) {
                Ok(ws) => ws,
                Err(frame::FrameError::UnknownWorkspace(_)) => {
                    match root.create_workspace(workspace) {
                        Ok(ws) => ws,
                        Err(err) => return fail(err),
                    }
                }
                Err(err) => return fail(err),
            };
            let member = if *clock {
                MemberRef::clock(session)
            } else if *log {
                MemberRef::log(format!("{session}-log"), session)
            } else if *pty {
                MemberRef::pty(session)
            } else if *edit {
                MemberRef::edit(session)
            } else {
                MemberRef::session(session)
            };
            let label = member.label();
            ws.add_member(member);
            match root.save_workspace(&ws) {
                Ok(()) => {
                    println!("{workspace}\t{label}");
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            }
        }
        WorkspaceCmd::Rm { name } => match root.delete_workspace(name) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => fail(err),
        },
    }
}

fn pty_cmd(root: Option<&std::path::Path>, cmd: &PtyCmd) -> ExitCode {
    let root = match open_root(root) {
        Ok(r) => r,
        Err(err) => return fail(err),
    };
    match cmd {
        PtyCmd::New { name, workspace } => {
            let workspace = workspace.as_deref().unwrap_or("default");
            if !root.workspace_exists(workspace) {
                if let Err(err) = root.create_workspace(workspace) {
                    return fail(err);
                }
            }
            let mut ws = match root.workspace(workspace) {
                Ok(ws) => ws,
                Err(err) => return fail(err),
            };
            ws.add_member(MemberRef::pty(name));
            match root.save_workspace(&ws) {
                Ok(()) => {
                    println!("{workspace}\t{name} · pty");
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            }
        }
        PtyCmd::List => {
            let mut names = Vec::new();
            match root.list_workspaces() {
                Ok(list) => {
                    for ws in list {
                        for member in ws.members {
                            if member.is_pty() && !names.iter().any(|n| n == member.id()) {
                                names.push(member.id().to_string());
                            }
                        }
                    }
                }
                Err(err) => return fail(err),
            }
            let hot = match anvil::serve::Client::connect(anvil::serve::default_sock()) {
                Ok(mut c) => match c.inspect() {
                    Ok(report) => report
                        .services
                        .into_iter()
                        .filter(|s| s.kind == "pty" && s.state == "hot")
                        .map(|s| s.name)
                        .collect::<Vec<_>>(),
                    Err(_) => Vec::new(),
                },
                Err(_) => Vec::new(),
            };
            if names.is_empty() && hot.is_empty() {
                return ExitCode::SUCCESS;
            }
            for name in &hot {
                if !names.iter().any(|n| n == name) {
                    names.push(name.clone());
                }
            }
            names.sort();
            for name in names {
                let state = if hot.iter().any(|h| h == &name) {
                    "hot"
                } else {
                    "cold"
                };
                println!("{name}\tpty\t{state}");
            }
            ExitCode::SUCCESS
        }
        PtyCmd::Snap { name } => {
            let mut c = match anvil::serve::Client::connect(anvil::serve::default_sock()) {
                Ok(c) => c,
                Err(_) => return fail("serve is down — start smith or `anvil serve`"),
            };
            match c.pty_snap(name) {
                Ok(screen) => {
                    print_pty_screen(&screen);
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            }
        }
        PtyCmd::Write { name, text, raw } => {
            let mut payload = if text.is_empty() {
                let mut buf = String::new();
                if let Err(err) = io::stdin().read_to_string(&mut buf) {
                    return fail(err);
                }
                buf
            } else {
                text.join(" ")
            };
            if !raw && !payload.ends_with('\r') && !payload.ends_with('\n') {
                payload.push('\r');
            }
            let mut c = match anvil::serve::Client::connect(anvil::serve::default_sock()) {
                Ok(c) => c,
                Err(_) => return fail("serve is down — start smith or `anvil serve`"),
            };
            if let Err(err) = c.pty_write(name, &payload) {
                return fail(err);
            }
            match c.pty_snap(name) {
                Ok(screen) => {
                    print_pty_screen(&screen);
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            }
        }
    }
}

fn print_pty_screen(screen: &anvil::serve::PtyScreen) {
    for line in &screen.lines {
        println!("{}", line.trim_end());
    }
}

fn edit_cmd(root: Option<&std::path::Path>, cmd: &EditCmd) -> ExitCode {
    let root = match open_root(root) {
        Ok(r) => r,
        Err(err) => return fail(err),
    };
    match cmd {
        EditCmd::New { name, workspace } => {
            let workspace = workspace.as_deref().unwrap_or("default");
            if !root.workspace_exists(workspace) {
                if let Err(err) = root.create_workspace(workspace) {
                    return fail(err);
                }
            }
            let mut ws = match root.workspace(workspace) {
                Ok(ws) => ws,
                Err(err) => return fail(err),
            };
            ws.add_member(MemberRef::edit(name));
            match root.save_workspace(&ws) {
                Ok(()) => {
                    println!("{workspace}\t{name} · edit");
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            }
        }
        EditCmd::Snap { name } => {
            if let Ok(mut c) = anvil::serve::Client::connect(anvil::serve::default_sock()) {
                return match c.edit_snap(name) {
                    Ok(buf) => {
                        print!("{}", buf.text);
                        if !buf.text.ends_with('\n') && !buf.text.is_empty() {
                            println!();
                        }
                        ExitCode::SUCCESS
                    }
                    Err(err) => fail(err),
                };
            }
            match root.edit_path(name) {
                Ok(path) => match std::fs::read_to_string(path) {
                    Ok(text) => {
                        print!("{text}");
                        ExitCode::SUCCESS
                    }
                    Err(err) if err.kind() == io::ErrorKind::NotFound => ExitCode::SUCCESS,
                    Err(err) => fail(err),
                },
                Err(err) => fail(err),
            }
        }
        EditCmd::Write { name, text } => {
            let payload = if text.is_empty() {
                let mut buf = String::new();
                if let Err(err) = io::stdin().read_to_string(&mut buf) {
                    return fail(err);
                }
                buf
            } else {
                let mut s = text.join(" ");
                if !s.ends_with('\n') {
                    s.push('\n');
                }
                s
            };
            if let Ok(mut c) = anvil::serve::Client::connect(anvil::serve::default_sock()) {
                if let Err(err) = c.edit(name, anvil::serve::EditOp::Set, &payload) {
                    return fail(err);
                }
                println!("{name}\t{} bytes", payload.len());
                return ExitCode::SUCCESS;
            }
            match root.edit_path(name) {
                Ok(path) => {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match std::fs::write(&path, payload.as_bytes()) {
                        Ok(()) => {
                            println!("{name}\t{} bytes", payload.len());
                            ExitCode::SUCCESS
                        }
                        Err(err) => fail(err),
                    }
                }
                Err(err) => fail(err),
            }
        }
    }
}

fn catalog_cmd(root: Option<&std::path::Path>, cmd: &CatalogCmd) -> ExitCode {
    let root = match open_root(root) {
        Ok(r) => r,
        Err(err) => return fail(err),
    };
    match cmd {
        CatalogCmd::List => match root.list_catalogs() {
            Ok(list) => {
                for cat in list {
                    println!("{}\t{}", cat.name, cat.workspaces.join(","));
                }
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        },
        CatalogCmd::New { name } => match root.create_catalog(name) {
            Ok(cat) => {
                println!("{}", cat.name);
                ExitCode::SUCCESS
            }
            Err(err) => fail(err),
        },
        CatalogCmd::Add { catalog, workspace } => {
            if !root.workspace_exists(workspace) {
                if let Err(err) = root.create_workspace(workspace) {
                    return fail(err);
                }
            }
            let mut cat = match root.catalog(catalog) {
                Ok(cat) => cat,
                Err(frame::FrameError::UnknownCatalog(_)) => match root.create_catalog(catalog) {
                    Ok(cat) => cat,
                    Err(err) => return fail(err),
                },
                Err(err) => return fail(err),
            };
            cat.add_workspace(workspace);
            match root.save_catalog(&cat) {
                Ok(()) => {
                    println!("{catalog}\t{workspace}");
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            }
        }
        CatalogCmd::Rm { name } => match root.delete_catalog(name) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => fail(err),
        },
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
    match complete::complete_messages_timed(prov, &model, &[("user", prompt.as_str())]) {
        Ok(stats) => {
            let t = stats.timing.compact();
            if !t.is_empty() {
                eprintln!("# {t}");
            }
            println!("{}", stats.text);
            ExitCode::SUCCESS
        }
        Err(err) => fail(err),
    }
}

fn ask_cmd(
    cli: &Cli,
    store: &std::path::Path,
    hammer: &std::path::Path,
    provider: Option<&str>,
    model: Option<&str>,
    prompt: &[String],
) -> ExitCode {
    let prompt = prompt.join(" ");
    if prompt.trim().is_empty() {
        return fail("ask needs a prompt");
    }
    if cli.store.is_none() {
        if let Ok(mut c) = anvil::serve::Client::connect(anvil::serve::default_sock()) {
            return match c.ask(&session_name(cli), &prompt, provider, model, &mut ()) {
                Ok(answer) => {
                    println!("{answer}");
                    ExitCode::SUCCESS
                }
                Err(err) => fail(err),
            };
        }
    }
    let (_cfg_path, cfg) = match load_cfg(cli.config.as_deref()) {
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
    let mut anvil = match Anvil::open(store, hammer) {
        Ok(a) => a,
        Err(err) => return fail(err),
    };
    let mut llm = HttpCompleter {
        provider: prov.clone(),
        model,
    };
    let log = open_root(cli.root.as_deref())
        .ok()
        .and_then(|r| r.load_events(&session_name(cli)).ok())
        .unwrap_or_default();
    match ask::ask_with_log(&mut llm, &mut anvil, &prompt, &log, &mut ()) {
        Ok(result) => {
            let t = result.timing.compact();
            if t.is_empty() {
                eprintln!("# strike {} turn(s)", result.turns);
            } else {
                eprintln!("# {} turn(s)  {t}", result.turns);
            }
            println!("{}", result.answer);
            ExitCode::SUCCESS
        }
        Err(err) => fail(err),
    }
}

fn fail(err: impl std::fmt::Display) -> ExitCode {
    eprintln!("anvil: {err}");
    ExitCode::from(1)
}
