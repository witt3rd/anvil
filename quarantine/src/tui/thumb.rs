//! Image thumbnails for the paste preview card.
//! Half-blocks everywhere; Kitty graphics on Ghostty/kitty (sharp).

use std::io::{self, Write};

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

const KITTY_ID: u32 = 17;

pub struct KittyBlit {
    pub area: Rect,
    pub png: Vec<u8>,
}

pub fn kitty_supported() -> bool {
    if std::env::var_os("KITTY_WINDOW_ID").is_some()
        || std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some()
        || std::env::var_os("GHOSTTY_BIN").is_some()
    {
        return true;
    }
    match std::env::var("TERM").unwrap_or_default().as_str() {
        "" | "dumb" | "linux" | "vt100" | "ansi" => false,
        term => {
            let prog = std::env::var("TERM_PROGRAM").unwrap_or_default();
            term.contains("kitty")
                || term.contains("ghostty")
                || term.contains("xterm")
                || prog.eq_ignore_ascii_case("ghostty")
                || prog.eq_ignore_ascii_case("kitty")
        }
    }
}

pub fn png_for_cells(bytes: &[u8], cols: u16, rows: u16) -> Option<Vec<u8>> {
    // Kitty scales to the cell box. If the box already matches the
    // photo's aspect, send the original so we don't stretch twice.
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) && bytes.len() < 2_000_000 {
        return Some(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes).ok()?;
    let max_w = u32::from(cols.max(1)).saturating_mul(16);
    let max_h = u32::from(rows.max(1)).saturating_mul(32);
    let resized = img.resize(max_w, max_h, image::imageops::FilterType::CatmullRom);
    let mut out = Vec::new();
    resized
        .write_to(
            &mut std::io::Cursor::new(&mut out),
            image::ImageFormat::Png,
        )
        .ok()?;
    Some(out)
}

pub fn kitty_place(area: Rect, png: &[u8]) -> bool {
    if area.width == 0 || area.height == 0 || png.is_empty() {
        return false;
    }
    let payload = super::clip::b64(png);
    let mut out = io::stdout();
    let ok = (|| {
        write!(
            out,
            "\x1b[s\x1b[{};{}H",
            area.y.saturating_add(1),
            area.x.saturating_add(1)
        )?;
        const CHUNK: usize = 4096;
        let mut off = 0;
        while off < payload.len() {
            let end = (off + CHUNK).min(payload.len());
            let more = u8::from(end < payload.len());
            if off == 0 {
                write!(
                    out,
                    "\x1b_Ga=T,f=100,i={KITTY_ID},c={},r={},C=1,q=2,m={more};{}\x1b\\",
                    area.width,
                    area.height,
                    &payload[off..end]
                )?;
            } else {
                write!(out, "\x1b_Gm={more};{}\x1b\\", &payload[off..end])?;
            }
            off = end;
        }
        write!(out, "\x1b[u")?;
        out.flush()
    })()
    .is_ok();
    ok
}

pub fn kitty_clear() -> bool {
    let mut out = io::stdout();
    write!(out, "\x1b_Ga=d,d=I,i={KITTY_ID},q=2\x1b\\").is_ok() && out.flush().is_ok()
}

pub fn halfblocks(bytes: &[u8], cols: u16, rows: u16) -> Option<Vec<Line<'static>>> {
    let img = image::load_from_memory(bytes).ok()?.to_rgb8();
    let cols = cols.max(2) as u32;
    let pix_h = (rows.max(1) as u32).saturating_mul(2);
    let resized =
        image::imageops::resize(&img, cols, pix_h, image::imageops::FilterType::Triangle);
    let mut lines = Vec::with_capacity(rows as usize);
    for y in 0..rows {
        let mut spans = Vec::with_capacity(cols as usize);
        let y0 = y as u32 * 2;
        let y1 = y0 + 1;
        for x in 0..cols {
            let image::Rgb([r0, g0, b0]) = *resized.get_pixel(x, y0);
            let lower = if y1 < pix_h {
                let image::Rgb([r1, g1, b1]) = *resized.get_pixel(x, y1);
                Color::Rgb(r1, g1, b1)
            } else {
                Color::Reset
            };
            spans.push(Span::styled(
                "▀",
                Style::default().fg(Color::Rgb(r0, g0, b0)).bg(lower),
            ));
        }
        lines.push(Line::from(spans));
    }
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png() -> Vec<u8> {
        // 1x1 red PNG.
        let mut img = image::RgbImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        img.put_pixel(1, 0, image::Rgb([0, 255, 0]));
        img.put_pixel(0, 1, image::Rgb([0, 0, 255]));
        img.put_pixel(1, 1, image::Rgb([255, 255, 0]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Png,
            )
            .unwrap();
        out
    }

    #[test]
    fn halfblocks_fill_the_grid() {
        let lines = halfblocks(&tiny_png(), 4, 2).expect("decode");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans.len(), 4);
        assert_eq!(lines[0].spans[0].content.as_ref(), "▀");
    }

    #[test]
    fn png_for_cells_encodes() {
        let png = png_for_cells(&tiny_png(), 8, 4).expect("encode");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    }
}
