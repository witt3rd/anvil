//! Keyboard mode the child asked for: kitty CSI u, or xterm
//! modifyOtherKeys. Scanned from the PTY bytes the process writes.

/// How the process wants keys encoded.
#[derive(Debug, Default)]
pub struct Mode {
    stack: Vec<u16>,
    modify: u8,
    rest: Vec<u8>,
}

impl Mode {
    pub fn feed(&mut self, bytes: &[u8]) {
        self.rest.extend_from_slice(bytes);
        let mut i = 0;
        while i < self.rest.len() {
            if self.rest[i] != 0x1b {
                i += 1;
                continue;
            }
            if i + 1 >= self.rest.len() {
                break;
            }
            if self.rest[i + 1] != b'[' {
                i += 1;
                continue;
            }
            let mut j = i + 2;
            while j < self.rest.len() {
                let b = self.rest[j];
                if (0x40..=0x7e).contains(&b) {
                    let params = self.rest[i + 2..j].to_vec();
                    self.apply(&params, b);
                    i = j + 1;
                    break;
                }
                j += 1;
            }
            if j >= self.rest.len() {
                break;
            }
        }
        self.rest.drain(..i);
        if self.rest.len() > 64 {
            self.rest.clear();
        }
    }

    /// Kitty flags (CSI > flags u). Zero means the process has not asked.
    pub fn kitty(&self) -> u16 {
        self.stack.last().copied().unwrap_or(0)
    }

    /// xterm modifyOtherKeys (CSI > 4 ; 1/2 m).
    pub fn modify(&self) -> bool {
        self.modify > 0
    }

    fn apply(&mut self, params: &[u8], end: u8) {
        match (end, params.first().copied()) {
            (b'u', Some(b'>')) => {
                let n = parse_u16(&params[1..]).unwrap_or(1);
                self.stack.push(n);
            }
            (b'u', Some(b'<')) => {
                let _ = self.stack.pop();
            }
            (b'u', Some(b'=')) => {
                let flags = params[1..]
                    .split(|c| *c == b';')
                    .next()
                    .and_then(parse_u16)
                    .unwrap_or(1);
                if !self.stack.is_empty() {
                    self.stack.pop();
                }
                self.stack.push(flags);
            }
            (b'm', _) if params.starts_with(b">4;") => {
                self.modify = parse_u16(&params[3..]).unwrap_or(0).min(2) as u8;
            }
            (b'm', _) if params == b">4" => self.modify = 0,
            _ => {}
        }
    }
}

fn parse_u16(bytes: &[u8]) -> Option<u16> {
    if bytes.is_empty() {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_push_and_pop() {
        let mut m = Mode::default();
        m.feed(b"\x1b[>1u");
        assert_eq!(m.kitty(), 1);
        m.feed(b"\x1b[>5u");
        assert_eq!(m.kitty(), 5);
        m.feed(b"\x1b[<u");
        assert_eq!(m.kitty(), 1);
        m.feed(b"\x1b[<u");
        assert_eq!(m.kitty(), 0);
    }

    #[test]
    fn kitty_split_across_reads() {
        let mut m = Mode::default();
        m.feed(b"\x1b[>");
        assert_eq!(m.kitty(), 0);
        m.feed(b"1u");
        assert_eq!(m.kitty(), 1);
    }

    #[test]
    fn modify_other_keys() {
        let mut m = Mode::default();
        m.feed(b"\x1b[>4;2m");
        assert!(m.modify());
        m.feed(b"\x1b[>4;0m");
        assert!(!m.modify());
    }
}
