//! Notice a catalog agent that started inside a shell, not via spawn.
//! Matching is the catalog's `adopt` list — not a table of brands.

use std::fs;

use crate::catalog::{AdoptHit, Agents};

/// Walk `root` and its children for a catalog row.
pub fn detect(root: u32, catalog: &Agents) -> Option<AdoptHit> {
    let mut best: Option<AdoptHit> = None;
    for pid in std::iter::once(root).chain(descendants(root)) {
        let cmd = cmdline(pid);
        let Some(mut hit) = catalog.adopt_cmd(&cmd) else {
            continue;
        };
        if hit.watch.is_none() && hit.listen {
            hit.watch = listen_in_tree(pid);
        }
        best = Some(merge_hit(best.take(), hit));
    }
    if let Some(hit) = best.as_mut() {
        if hit.watch.is_none() && hit.listen {
            hit.watch = listen_in_tree(root);
        }
    }
    best
}

fn merge_hit(best: Option<AdoptHit>, hit: AdoptHit) -> AdoptHit {
    let Some(mut best) = best else {
        return hit;
    };
    if hit.name.len() > best.name.len() {
        let watch = hit.watch.clone().or(best.watch.take());
        let session = hit.session.clone().or(best.session.take());
        return AdoptHit {
            watch,
            session,
            ..hit
        };
    }
    if best.watch.is_none() {
        best.watch = hit.watch;
    }
    if best.session.is_none() {
        best.session = hit.session;
    }
    best
}

fn listen_in_tree(root: u32) -> Option<String> {
    std::iter::once(root)
        .chain(descendants(root))
        .find_map(listen_port)
        .map(|p| format!("http://127.0.0.1:{p}"))
}

pub fn cwd(pid: u32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
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
    fn proc_net_port_is_the_hex_after_the_colon() {
        assert_eq!(parse_hex_port("0100007F:1F90"), Some(8080));
        assert_eq!(
            parse_hex_port("00000000000000000000000001000000:0FA0"),
            Some(4000)
        );
    }
}
