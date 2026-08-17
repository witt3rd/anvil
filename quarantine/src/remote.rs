//! SSH is the inter-machine bus. One host per casing first.

use std::process::Command;

/// Pull `--remote HOST` / `-R HOST` / `--remote=HOST` out of argv.
pub fn strip_remote<I>(args: I) -> (Option<String>, Vec<String>)
where
    I: IntoIterator<Item = String>,
{
    let mut remote = None;
    let mut out = Vec::new();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        if arg == "--remote" || arg == "-R" {
            remote = it.next();
            continue;
        }
        if let Some(host) = arg.strip_prefix("--remote=") {
            remote = Some(host.to_string());
            continue;
        }
        out.push(arg);
    }
    (remote, out)
}

/// `ssh [-t] host -- bin args…`. Returns the process exit code.
pub fn exec(host: &str, bin: &str, args: &[String], tty: bool) -> i32 {
    let mut cmd = Command::new("ssh");
    if tty {
        cmd.arg("-t");
    }
    cmd.arg(host).arg("--").arg(bin).args(args);
    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("anvil: ssh {host}: {err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_remote_long_and_short() {
        let (r, rest) = strip_remote(
            ["inspect", "--remote", "prince", "--json"]
                .into_iter()
                .map(str::to_string),
        );
        assert_eq!(r.as_deref(), Some("prince"));
        assert_eq!(rest, ["inspect", "--json"]);
        let (r, rest) = strip_remote(
            ["-R", "king", "serve", "--status"]
                .into_iter()
                .map(str::to_string),
        );
        assert_eq!(r.as_deref(), Some("king"));
        assert_eq!(rest, ["serve", "--status"]);
        let (r, rest) = strip_remote(
            ["--remote=chef", "session", "list"]
                .into_iter()
                .map(str::to_string),
        );
        assert_eq!(r.as_deref(), Some("chef"));
        assert_eq!(rest, ["session", "list"]);
    }
}
