//! Ask-compose paste chips. Grok-build: `[Pasted: N lines]` / `[Image #N]`.

use std::path::PathBuf;

use super::clip;

/// Fewer than this many lines insert as raw text (grok-build).
pub const LINE_THRESHOLD: usize = 5;

#[derive(Debug, Clone)]
pub enum Paste {
    Text {
        body: String,
    },
    Image {
        mime: String,
        bytes: Vec<u8>,
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipSpan {
    pub start: usize,
    pub end: usize,
    pub index: usize,
}

impl Paste {
    pub fn text(body: impl Into<String>) -> Self {
        Self::Text { body: body.into() }
    }

    pub fn image(mime: impl Into<String>, bytes: Vec<u8>, path: Option<PathBuf>) -> Self {
        Self::Image {
            mime: mime.into(),
            bytes,
            path,
        }
    }

    pub fn is_image(&self) -> bool {
        matches!(self, Self::Image { .. })
    }

    pub fn chip(&self, image_n: usize) -> String {
        match self {
            Self::Text { body } => {
                let n = count_lines(body);
                format!("[Pasted: {n} lines]")
            }
            Self::Image { .. } => format!("[Image #{image_n}]"),
        }
    }

    pub fn expand(&self) -> String {
        match self {
            Self::Text { body } => body.clone(),
            Self::Image { path, .. } => path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "[image]".into()),
        }
    }
}

pub fn count_lines(text: &str) -> usize {
    text.lines().count().max(1)
}

pub fn image_n(pastes: &[Paste], index: usize) -> usize {
    pastes
        .get(..=index)
        .map(|slice| slice.iter().filter(|p| p.is_image()).count())
        .unwrap_or(0)
}

pub fn chip_of(pastes: &[Paste], index: usize) -> String {
    pastes[index].chip(image_n(pastes, index))
}

pub fn chip_spans(input: &str, pastes: &[Paste]) -> Vec<ChipSpan> {
    let mut from = 0;
    let mut out = Vec::new();
    for i in 0..pastes.len() {
        let chip = chip_of(pastes, i);
        if let Some(pos) = input[from..].find(&chip) {
            let start = from + pos;
            let end = start + chip.len();
            out.push(ChipSpan {
                start,
                end,
                index: i,
            });
            from = end;
        }
    }
    out
}

pub fn chip_at(input: &str, cursor: usize, pastes: &[Paste]) -> Option<usize> {
    let spans = chip_spans(input, pastes);
    spans
        .iter()
        .rev()
        .find(|s| cursor > s.start && cursor <= s.end)
        .or_else(|| spans.iter().rev().find(|s| cursor == s.end))
        .map(|s| s.index)
}

pub fn chip_covering(input: &str, cursor: usize, pastes: &[Paste]) -> Option<ChipSpan> {
    chip_spans(input, pastes)
        .into_iter()
        .rev()
        .find(|s| cursor > s.start && cursor <= s.end)
}

pub fn expand(input: &str, pastes: &[Paste]) -> String {
    let mut result = input.to_string();
    for span in chip_spans(input, pastes).into_iter().rev() {
        let body = pastes[span.index].expand();
        if span.end <= result.len() {
            result.replace_range(span.start..span.end, &body);
        }
    }
    result
}

/// Same payload pasted again expands the matching chip (grok-build).
pub fn try_expand_matching(
    input: &mut String,
    cursor: &mut usize,
    pastes: &mut Vec<Paste>,
    incoming: &Paste,
) -> bool {
    let Some(index) = pastes.iter().rposition(|p| same(p, incoming)) else {
        return false;
    };
    let chip = chip_of(pastes, index);
    let Some(start) = input.rfind(&chip) else {
        return false;
    };
    let body = pastes[index].expand();
    input.replace_range(start..start + chip.len(), &body);
    *cursor = start + body.len();
    pastes.remove(index);
    true
}

fn same(a: &Paste, b: &Paste) -> bool {
    match (a, b) {
        (Paste::Text { body: x }, Paste::Text { body: y }) => x == y,
        (
            Paste::Image {
                bytes: x, mime: mx, ..
            },
            Paste::Image {
                bytes: y, mime: my, ..
            },
        ) => x == y && mx == my,
        _ => false,
    }
}

pub fn ingest_text(text: String) -> Option<Paste> {
    if let Some(path) = as_existing_file(&text) {
        if let Some(mime) = clip::image_mime(&path) {
            if let Ok(bytes) = std::fs::read(&path) {
                return Some(Paste::image(mime, bytes, Some(path)));
            }
        }
    }
    if count_lines(&text) < LINE_THRESHOLD {
        return None;
    }
    Some(Paste::text(text))
}

pub fn persist_image(image: clip::Image) -> Paste {
    let path = write_temp_image(&image);
    Paste::image(image.mime, image.bytes, path)
}

fn write_temp_image(image: &clip::Image) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("anvil-pastes");
    std::fs::create_dir_all(&dir).ok()?;
    let ext = match image.mime.as_str() {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    };
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("image-{n}.{ext}"));
    std::fs::write(&path, &image.bytes).ok()?;
    Some(path)
}

fn as_existing_file(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    let path = if let Some(rest) = trimmed.strip_prefix("file://") {
        PathBuf::from(rest)
    } else {
        PathBuf::from(trimmed)
    };
    path.is_file().then_some(path)
}

pub struct Preview {
    pub lines: Vec<String>,
    pub more: usize,
    pub footer_lead: &'static str,
    pub footer_or: &'static str,
    pub footer_tail: &'static str,
}

pub fn preview(paste: &Paste, image_n: usize) -> Preview {
    const SHOW: usize = 4;
    match paste {
        Paste::Text { body } => {
            let all: Vec<&str> = body.lines().collect();
            let lines = all.iter().take(SHOW).map(|s| (*s).to_string()).collect();
            let more = all.len().saturating_sub(SHOW);
            Preview {
                lines,
                more,
                footer_lead: "paste again",
                footer_or: " or ",
                footer_tail: "double-click to expand",
            }
        }
        Paste::Image { mime, bytes, path } => {
            let kind = clip::kind_label(mime);
            let size = clip::fmt_size(bytes.len());
            let dim = clip::dimensions(bytes)
                .map(|(w, h)| format!("{w}x{h}"))
                .unwrap_or_else(|| "?x?".into());
            let name = path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("image-{image_n}"));
            let mut lines = vec![format!(
                "Image #{image_n} — {kind} · {dim} · {size} · {name}"
            )];
            if let Some(p) = path {
                lines.push(format!("Path: {}", p.display()));
            }
            Preview {
                lines,
                more: 0,
                footer_lead: "paste again",
                footer_or: " or ",
                footer_tail: "double-click to expand",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn many_lines(n: usize) -> String {
        (1..=n)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn chip_matches_requested_shape() {
        let text = Paste::text(many_lines(42));
        assert_eq!(text.chip(1), "[Pasted: 42 lines]");
        let img = Paste::image("image/png", vec![1, 2, 3], None);
        assert_eq!(img.chip(2), "[Image #2]");
    }

    #[test]
    fn short_text_is_not_a_chip() {
        assert!(ingest_text("hello\nworld".into()).is_none());
        assert!(ingest_text(many_lines(5)).is_some());
    }

    #[test]
    fn expand_replaces_chips_in_order() {
        let pastes = vec![
            Paste::text(many_lines(5)),
            Paste::image("image/png", b"png".to_vec(), Some(PathBuf::from("/tmp/a.png"))),
        ];
        let input = format!(
            "see {} and {}",
            chip_of(&pastes, 0),
            chip_of(&pastes, 1)
        );
        let expanded = expand(&input, &pastes);
        assert!(expanded.contains("line 5"), "{expanded}");
        assert!(expanded.contains("/tmp/a.png"), "{expanded}");
        assert!(
            !expanded.contains("[Pasted:") && !expanded.contains("[Image #"),
            "{expanded}"
        );
    }

    #[test]
    fn paste_again_expands_matching_chip() {
        let body = many_lines(6);
        let mut pastes = vec![Paste::text(body.clone())];
        let chip = chip_of(&pastes, 0);
        let mut input = format!("pre {chip} post");
        let mut cursor = input.len();
        assert!(try_expand_matching(
            &mut input,
            &mut cursor,
            &mut pastes,
            &Paste::text(body.clone())
        ));
        assert!(input.contains("line 1"), "{input}");
        assert!(!input.contains("[Pasted:"), "{input}");
        assert!(pastes.is_empty());
    }

    #[test]
    fn chip_at_cursor_picks_the_left_chip() {
        let pastes = vec![Paste::text(many_lines(5)), Paste::text(many_lines(8))];
        let a = chip_of(&pastes, 0);
        let b = chip_of(&pastes, 1);
        let input = format!("{a}{b}");
        assert_eq!(chip_at(&input, a.len(), &pastes), Some(0));
        assert_eq!(chip_at(&input, input.len(), &pastes), Some(1));
    }

    #[test]
    fn preview_counts_remaining_lines() {
        let p = preview(&Paste::text(many_lines(10)), 1);
        assert_eq!(p.lines.len(), 4);
        assert_eq!(p.more, 6);
        assert_eq!(p.footer_lead, "paste again");
    }

    #[test]
    fn image_preview_names_the_slot() {
        let img = Paste::image(
            "image/png",
            vec![1, 2, 3],
            Some(PathBuf::from("/tmp/shot.png")),
        );
        let p = preview(&img, 2);
        assert!(p.lines[0].starts_with("Image #2 — PNG"), "{:?}", p.lines);
        assert!(p.lines.iter().any(|l| l.contains("Path: /tmp/shot.png")));
    }
}
