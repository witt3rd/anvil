//! Clipboard write. Prefer `wl-copy` / `xclip`, then OSC 52.

use std::io::Write;
use std::process::{Command, Stdio};

/// Write text to the system clipboard. True if a helper or OSC 52 took it.
pub fn write_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    write_via_helper(text.as_bytes()) || write_osc52(text)
}

pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", b64(text.as_bytes()))
}

fn write_via_helper(bytes: &[u8]) -> bool {
    for (prog, args) in [
        ("wl-copy", &[][..]),
        ("xclip", &["-selection", "clipboard"][..]),
        ("xsel", &["--clipboard", "--input"][..]),
    ] {
        if spawn_write(prog, args, bytes) {
            return true;
        }
    }
    false
}

fn write_osc52(text: &str) -> bool {
    let mut out = std::io::stdout();
    out.write_all(osc52(text).as_bytes()).is_ok() && out.flush().is_ok()
}

fn spawn_write(prog: &str, args: &[&str], bytes: &[u8]) -> bool {
    let mut child = match Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let ok = child
        .stdin
        .as_mut()
        .and_then(|stdin| stdin.write_all(bytes).ok())
        .is_some();
    matches!(child.wait(), Ok(status) if status.success()) && ok
}

fn b64(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = bytes.get(i + 1).copied();
        let b2 = bytes.get(i + 2).copied();
        out.push(T[(b0 >> 2) as usize] as char);
        out.push(T[(((b0 & 3) << 4) | (b1.unwrap_or(0) >> 4)) as usize] as char);
        match b1 {
            None => {
                out.push('=');
                out.push('=');
            }
            Some(b1) => {
                out.push(T[(((b1 & 0xf) << 2) | (b2.unwrap_or(0) >> 6)) as usize] as char);
                match b2 {
                    None => out.push('='),
                    Some(b2) => out.push(T[(b2 & 0x3f) as usize] as char),
                }
            }
        }
        i += 3;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_encodes_plain_text() {
        let seq = osc52("hi");
        assert!(seq.starts_with("\x1b]52;c;"), "{seq:?}");
        assert!(seq.ends_with('\u{7}'), "{seq:?}");
        assert!(seq.contains("aGk="), "{seq:?}");
    }
}
