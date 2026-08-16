//! Clipboard write/read. Prefer `wl-copy` / `xclip`, then OSC 52.

use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct Image {
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub fn write_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    write_via_helper(text.as_bytes()) || write_osc52(text)
}

pub fn read_text() -> Option<String> {
    for (prog, args) in [
        ("wl-paste", &["-n"][..]),
        ("xclip", &["-selection", "clipboard", "-out"][..]),
        ("xsel", &["--clipboard", "--output"][..]),
    ] {
        if let Some(text) = run_read(prog, args) {
            return Some(text);
        }
    }
    None
}

pub fn read_image() -> Option<Image> {
    for (mime, args) in [
        ("image/png", &["--type", "image/png"][..]),
        ("image/jpeg", &["--type", "image/jpeg"][..]),
        ("image/webp", &["--type", "image/webp"][..]),
    ] {
        if let Some(bytes) = run_read_bytes("wl-paste", args) {
            if looks_like_image(&bytes) {
                return Some(Image {
                    mime: mime.into(),
                    bytes,
                });
            }
        }
    }
    if let Some(bytes) = run_read_bytes(
        "xclip",
        &["-selection", "clipboard", "-t", "image/png", "-o"],
    ) {
        if looks_like_image(&bytes) {
            return Some(Image {
                mime: "image/png".into(),
                bytes,
            });
        }
    }
    None
}

pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", b64(text.as_bytes()))
}

pub fn kind_label(mime: &str) -> &'static str {
    match mime {
        "image/png" => "PNG",
        "image/jpeg" => "JPEG",
        "image/gif" => "GIF",
        "image/webp" => "WEBP",
        "image/bmp" => "BMP",
        _ => "image",
    }
}

pub fn fmt_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// `(width, height)` from a PNG/JPEG/GIF/WEBP header, if we can see it.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) && bytes.len() >= 24 {
        let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return (w > 0 && h > 0).then_some((w, h));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return jpeg_size(bytes);
    }
    if bytes.starts_with(b"GIF8") && bytes.len() >= 10 {
        let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        return (w > 0 && h > 0).then_some((w, h));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..16) == Some(b"WEBPVP8 ") && bytes.len() >= 30 {
        let w = (u16::from_le_bytes([bytes[26], bytes[27]]) as u32 & 0x3fff) + 1;
        let h = (u16::from_le_bytes([bytes[28], bytes[29]]) as u32 & 0x3fff) + 1;
        return Some((w, h));
    }
    None
}

fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xff {
            return None;
        }
        let marker = bytes[i + 1];
        i += 2;
        if marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if i + 1 >= bytes.len() {
            return None;
        }
        let len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
        if len < 2 || i + len > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0 | 0xc1 | 0xc2) && len >= 7 {
            let h = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]) as u32;
            let w = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            return (w > 0 && h > 0).then_some((w, h));
        }
        i += len;
    }
    None
}

pub fn image_mime(path: &std::path::Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("bmp") => Some("image/bmp"),
        _ => None,
    }
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

fn run_read(prog: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(prog)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

fn run_read_bytes(prog: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(prog)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    Some(out.stdout)
}

fn looks_like_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G'])
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(b"GIF8")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}

pub(crate) fn b64(bytes: &[u8]) -> String {
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

    #[test]
    fn image_mime_from_extension() {
        assert_eq!(
            image_mime(std::path::Path::new("/tmp/shot.PNG")),
            Some("image/png")
        );
        assert_eq!(image_mime(std::path::Path::new("notes.txt")), None);
    }

    #[test]
    fn png_signature_is_an_image() {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&[0; 8]);
        assert!(looks_like_image(&bytes));
        assert!(!looks_like_image(b"not an image"));
    }

    #[test]
    fn png_ihdr_dimensions() {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
        bytes.extend_from_slice(&1500u32.to_be_bytes());
        bytes.extend_from_slice(&486u32.to_be_bytes());
        assert_eq!(dimensions(&bytes), Some((1500, 486)));
        assert_eq!(fmt_size(317_645), "310.2 KB");
        assert_eq!(kind_label("image/png"), "PNG");
    }
}
