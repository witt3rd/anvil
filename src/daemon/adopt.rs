//! Notice a catalog agent that started inside a shell, not via spawn.

use std::fs;
use std::path::Path;

/// What a process tree looks like when it is a catalog agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub name: String,
    pub watch: Option<String>,
}

/// Walk `root` and its children for oc / oc-work / opencode / grok.
pub fn detect(root: u32) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for pid in std::iter::once(root).chain(descendants(root)) {
        let cmd = cmdline(pid);
        let Some(mut hit) = match_cmd(&cmd) else {
            continue;
        };
        if hit.watch.is_none() && hit.name != "grok" {
            if let Some(port) = listen_port(pid) {
                hit.watch = Some(format!("http://127.0.0.1:{port}"));
            }
        }
        if best.as_ref().is_none_or(|b| b.name != "oc-work") {
            best = Some(hit);
        }
    }
    best
}

pub fn match_cmd(cmd: &str) -> Option<Hit> {
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let bins: Vec<&str> = words.iter().filter_map(|w| file_name(w)).collect();
    let argv0 = bins.first().copied().unwrap_or("");
    let script = if argv0 == "bash" || argv0 == "sh" {
        bins.get(1).copied()
    } else {
        None
    };
    let name = if argv0 == "oc-work" || script == Some("oc-work") {
        "oc-work"
    } else if argv0 == "oc" || argv0 == "opencode" || script == Some("oc") {
        "oc"
    } else if argv0 == "grok" || script == Some("grok") {
        "grok"
    } else if argv0 == "node" && bins.iter().any(|b| *b == "grok") {
        "grok"
    } else {
        return None;
    };
    let watch = port_from_args(&words).map(|p| format!("http://127.0.0.1:{p}"));
    Some(Hit {
        name: name.into(),
        watch,
    })
}

pub fn port_from_args(words: &[&str]) -> Option<u16> {
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if let Some(rest) = w.strip_prefix("--port=") {
            return rest.parse().ok();
        }
        if w == "--port" {
            return words.get(i + 1)?.parse().ok();
        }
        i += 1;
    }
    None
}

fn file_name(path: &str) -> Option<&str> {
    Path::new(path).file_name()?.to_str()
}

fn cmdline(pid: u32) -> String {
    fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| String::from_utf8_lossy(&b).replace('\0', " "))
        .unwrap_or_default()
}

pub fn descendants(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut stack = children_of(pid);
    while let Some(child) = stack.pop() {
        out.push(child);
        stack.extend(children_of(child));
    }
    out
}

fn children_of(pid: u32) -> Vec<u32> {
    // Doppler and friends spawn on a worker thread; the main
    // task's children file is empty.
    let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for task in tasks.flatten() {
        let Ok(text) = fs::read_to_string(task.path().join("children")) else {
            continue;
        };
        for pid in text.split_whitespace().filter_map(|s| s.parse().ok()) {
            if !out.contains(&pid) {
                out.push(pid);
            }
        }
    }
    out
}

/// First listening TCP port for this pid (IPv4 or IPv6).
fn listen_port(pid: u32) -> Option<u16> {
    let inodes = socket_inodes(pid);
    if inodes.is_empty() {
        return None;
    }
    listen_in(&inodes, "/proc/net/tcp").or_else(|| listen_in(&inodes, "/proc/net/tcp6"))
}

fn listen_in(inodes: &[u64], path: &str) -> Option<u16> {
    let tcp = fs::read_to_string(path).ok()?;
    for line in tcp.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 {
            continue;
        }
        // st 0A = listen
        if cols[3] != "0A" {
            continue;
        }
        let inode: u64 = cols[9].parse().ok()?;
        if !inodes.contains(&inode) {
            continue;
        }
        return parse_hex_port(cols[1]);
    }
    None
}

/// `/proc/net/tcp{,6}` local address: hex-ip `:` hex-port.
fn parse_hex_port(local: &str) -> Option<u16> {
    u16::from_str_radix(local.rsplit_once(':')?.1, 16).ok()
}

fn socket_inodes(pid: u32) -> Vec<u64> {
    let Ok(entries) = fs::read_dir(format!("/proc/{pid}/fd")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Ok(link) = fs::read_link(entry.path()) else {
            continue;
        };
        let s = link.to_string_lossy();
        let Some(rest) = s.strip_prefix("socket:[") else {
            continue;
        };
        let Some(num) = rest.strip_suffix(']') else {
            continue;
        };
        if let Ok(n) = num.parse() {
            out.push(n);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_cmd_knows_the_wrappers() {
        let oc = match_cmd("oc --hostname 127.0.0.1 --port 4096").unwrap();
        assert_eq!(oc.name, "oc");
        assert_eq!(oc.watch.as_deref(), Some("http://127.0.0.1:4096"));

        let work = match_cmd("/home/dt/.local/bin/oc-work").unwrap();
        assert_eq!(work.name, "oc-work");
        assert!(work.watch.is_none());

        let raw = match_cmd("opencode").unwrap();
        assert_eq!(raw.name, "oc");

        let grok = match_cmd("grok").unwrap();
        assert_eq!(grok.name, "grok");
        assert!(grok.watch.is_none());

        let wrapped = match_cmd(
            "node /home/dt/.local/share/mise/installs/npm-xai-official-grok/latest/node_modules/@xai-official/grok/bin/grok",
        )
        .unwrap();
        assert_eq!(wrapped.name, "grok");
        assert!(match_cmd("git clone grok").is_none());

        let wrap = match_cmd("/bin/bash /home/dt/.local/bin/oc").unwrap();
        assert_eq!(wrap.name, "oc");
        let wrap_w = match_cmd("/bin/bash /home/dt/.local/bin/oc-work").unwrap();
        assert_eq!(wrap_w.name, "oc-work");

        assert!(match_cmd("sh").is_none());
        assert!(match_cmd("git clone opencode").is_none());
    }

    #[test]
    fn port_flag_forms() {
        assert_eq!(port_from_args(&["oc", "--port", "9"]), Some(9));
        assert_eq!(port_from_args(&["oc", "--port=11"]), Some(11));
        assert_eq!(port_from_args(&["oc"]), None);
    }

    #[test]
    fn proc_net_port_is_the_hex_after_the_colon() {
        assert_eq!(parse_hex_port("0100007F:1F90"), Some(8080));
        assert_eq!(parse_hex_port("00000000000000000000000001000000:0FA0"), Some(4000));
    }
}
