//! A small software renderer: a frame's draw list to an image, without a browser. Used for
//! playtest snapshots and contact sheets (servers, the desktop app, WebAssembly). The browser
//! player stays the real renderer; this one is faithful in layout and colour, with a compact
//! built-in pixel font for text.

use std::collections::BTreeMap;

use tiny_skia::{Color, FillRule, IntSize, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};

use crate::playtest::{Report, Snapshot};
use crate::world::{Draw, Frame};

/// Images by asset name, as PNG bytes (other formats draw as an outline).
pub type Images = BTreeMap<String, Vec<u8>>;

fn color(s: &str, alpha: f32) -> Color {
    let s = s.trim();
    let named = match s.to_ascii_lowercase().as_str() {
        "white" => Some((255, 255, 255)),
        "black" => Some((0, 0, 0)),
        "red" => Some((239, 68, 68)),
        "green" => Some((34, 197, 94)),
        "blue" => Some((59, 130, 246)),
        "yellow" | "gold" => Some((240, 185, 11)),
        "orange" => Some((249, 115, 22)),
        "purple" => Some((168, 85, 247)),
        "pink" => Some((236, 72, 153)),
        "gray" | "grey" => Some((128, 128, 128)),
        _ => None,
    };
    let (r, g, b, a) = if let Some((r, g, b)) = named {
        (r, g, b, 255)
    } else if let Some(h) = s.strip_prefix('#') {
        let p = |i: usize, n: usize| u8::from_str_radix(&h[i..i + n], 16).unwrap_or(255);
        match h.len() {
            3 => (p(0, 1) * 17, p(1, 1) * 17, p(2, 1) * 17, 255),
            6 => (p(0, 2), p(2, 2), p(4, 2), 255),
            8 => (p(0, 2), p(2, 2), p(4, 2), p(6, 2)),
            _ => (255, 255, 255, 255),
        }
    } else {
        (255, 255, 255, 255)
    };
    Color::from_rgba8(r, g, b, ((a as f32) * alpha.clamp(0.0, 1.0)) as u8)
}

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

/// 3×5 glyphs, rows top to bottom, 3 bits each (left = high bit).
fn glyph(ch: char) -> Option<[u8; 5]> {
    let g = match ch.to_ascii_uppercase() {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        ' ' => [0, 0, 0, 0, 0],
        '.' => [0, 0, 0, 0, 2],
        ',' => [0, 0, 0, 2, 4],
        ':' => [0, 2, 0, 2, 0],
        ';' => [0, 2, 0, 2, 4],
        '!' => [2, 2, 2, 0, 2],
        '?' => [7, 1, 2, 0, 2],
        '-' => [0, 0, 7, 0, 0],
        '+' => [0, 2, 7, 2, 0],
        '=' => [0, 7, 0, 7, 0],
        '/' => [1, 1, 2, 4, 4],
        '(' => [2, 4, 4, 4, 2],
        ')' => [2, 1, 1, 1, 2],
        '%' => [5, 1, 2, 4, 5],
        '\'' => [2, 2, 0, 0, 0],
        '"' => [5, 5, 0, 0, 0],
        '#' => [5, 7, 5, 7, 5],
        '<' => [1, 2, 4, 2, 1],
        '>' => [4, 2, 1, 2, 4],
        '_' => [0, 0, 0, 0, 7],
        '*' => [5, 2, 5, 0, 0],
        '$' => [3, 6, 2, 3, 6],
        '&' => [2, 5, 2, 5, 3],
        _ => return None,
    };
    Some(g)
}

/// Text in the pixel font, `px` tall per glyph row (a glyph is 5 rows, 4 columns with spacing).
fn text(pm: &mut Pixmap, s: &str, x: f32, y: f32, px: f32, c: Color, t: Transform) {
    let p = paint(c);
    let mut cx = x;
    for ch in s.chars() {
        let g = glyph(ch).unwrap_or([7, 5, 5, 5, 7]);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    if let Some(r) = Rect::from_xywh(cx + col as f32 * px, y + row as f32 * px, px, px) {
                        pm.fill_rect(r, &p, t, None);
                    }
                }
            }
        }
        cx += 4.0 * px;
    }
}

fn text_width(s: &str, px: f32) -> f32 {
    s.chars().count() as f32 * 4.0 * px - px
}

/// Draw one frame at the game's resolution `size`, scaled by `scale`.
pub fn draw_frame(frame: &Frame, size: [f32; 2], scale: f32, images: &Images) -> Option<Pixmap> {
    let (w, h) = ((size[0] * scale).round().max(1.0) as u32, (size[1] * scale).round().max(1.0) as u32);
    let mut pm = Pixmap::new(w, h)?;
    pm.fill(color(&frame.background, 1.0));
    let base = Transform::from_scale(scale, scale);
    let mut decoded: BTreeMap<&str, Option<Pixmap>> = BTreeMap::new();
    for d in &frame.draws {
        match d {
            Draw::Rect { x, y, w, h, c, a, rot } => {
                let t = base.pre_concat(Transform::from_rotate_at(*rot, x + w / 2.0, y + h / 2.0));
                if let Some(r) = Rect::from_xywh(*x, *y, w.max(0.01), h.max(0.01)) {
                    pm.fill_rect(r, &paint(color(c, *a)), t, None);
                }
            }
            Draw::Circle { x, y, w, h, c, a } => {
                if let Some(path) = Rect::from_xywh(*x, *y, w.max(0.01), h.max(0.01)).and_then(PathBuilder::from_oval) {
                    pm.fill_path(&path, &paint(color(c, *a)), FillRule::Winding, base, None);
                }
            }
            Draw::Image { img, f, fw, fh, x, y, w, h, a, rot, flip } => {
                let sheet = decoded.entry(img.as_str()).or_insert_with(|| images.get(img).and_then(|b| Pixmap::decode_png(b).ok()));
                let Some(sheet) = sheet else {
                    // Not available here: an outline where it would be.
                    if let Some(r) = Rect::from_xywh(*x, *y, w.max(1.0), h.max(1.0)) {
                        let mut pb = PathBuilder::new();
                        pb.push_rect(r);
                        if let Some(path) = pb.finish() {
                            let stroke = tiny_skia::Stroke { width: 1.0, ..Default::default() };
                            pm.stroke_path(&path, &paint(Color::from_rgba8(255, 255, 255, 90)), &stroke, base, None);
                        }
                    }
                    continue;
                };
                // The frame `f` of the sheet (or the whole image).
                let (sw, sh) = (sheet.width() as f32, sheet.height() as f32);
                let (cw, ch) = if *fw > 0.0 && *fh > 0.0 { (*fw, *fh) } else { (sw, sh) };
                let cols = (sw / cw).floor().max(1.0) as u32;
                let (fx, fy) = ((f % cols) as f32 * cw, (f / cols) as f32 * ch);
                let Some(size) = IntSize::from_wh(cw as u32, ch as u32) else { continue };
                let Some(rect) = tiny_skia::IntRect::from_xywh(fx as i32, fy as i32, size.width(), size.height()) else { continue };
                let Some(cell) = sheet.clone_rect(rect) else { continue };
                let mut t = Transform::from_translate(*x, *y).pre_scale(w / cw, h / ch);
                if *flip {
                    t = t.pre_translate(cw, 0.0).pre_scale(-1.0, 1.0);
                }
                t = Transform::from_rotate_at(*rot, x + w / 2.0, y + h / 2.0).pre_concat(t);
                let pp = PixmapPaint { opacity: a.clamp(0.0, 1.0), quality: tiny_skia::FilterQuality::Nearest, ..Default::default() };
                pm.draw_pixmap(0, 0, cell.as_ref(), &pp, base.pre_concat(t), None);
            }
            Draw::Text { s, x, y, w, size, c, align, a } => {
                let px = (size / 6.0).max(1.0);
                let tw = text_width(s, px);
                let tx = match align.as_str() {
                    "center" => x + (w - tw) / 2.0,
                    "right" => x + w - tw,
                    _ => *x,
                };
                text(&mut pm, s, tx, *y + (size - 5.0 * px) / 2.0, px, color(c, *a), base);
            }
        }
    }
    Some(pm)
}

/// One frame as a PNG.
pub fn frame_png(frame: &Frame, size: [f32; 2], scale: f32, images: &Images) -> Option<Vec<u8>> {
    draw_frame(frame, size, scale, images)?.encode_png().ok()
}

/// A contact sheet of snapshots: one row per bot run, labelled, as a PNG.
pub fn contact_sheet(report: &Report, size: [f32; 2], images: &Images) -> Option<Vec<u8>> {
    let cell_w = 240.0f32;
    let scale = cell_w / size[0].max(1.0);
    let cell_h = (size[1] * scale).round();
    let cols = report.runs.iter().map(|r| r.snapshots.len()).max().unwrap_or(1).max(1);
    let (pad, label) = (12.0f32, 22.0f32);
    let width = pad + cols as f32 * (cell_w + pad) + 90.0;
    let height = pad + report.runs.len() as f32 * (cell_h + label + pad);
    let mut sheet = Pixmap::new(width as u32, height as u32)?;
    sheet.fill(Color::from_rgba8(10, 10, 11, 255));
    let fg = Color::from_rgba8(180, 180, 187, 255);
    let gold = Color::from_rgba8(240, 185, 11, 255);
    for (ri, run) in report.runs.iter().enumerate() {
        let y0 = pad + ri as f32 * (cell_h + label + pad);
        text(&mut sheet, &run.bot.to_uppercase(), pad, y0 + label + cell_h / 2.0 - 6.0, 2.0, gold, Transform::identity());
        let status = format!("{:?}", run.status).to_uppercase();
        text(&mut sheet, &status, pad, y0 + label + cell_h / 2.0 + 8.0, 1.5, fg, Transform::identity());
        for (ci, snap) in run.snapshots.iter().enumerate() {
            let x0 = 90.0 + ci as f32 * (cell_w + pad);
            draw_cell(&mut sheet, snap, size, scale, images, x0, y0 + label);
            text(&mut sheet, &format!("{:.1}S", snap.time), x0, y0 + 6.0, 2.0, fg, Transform::identity());
        }
    }
    sheet.encode_png().ok()
}

fn draw_cell(sheet: &mut Pixmap, snap: &Snapshot, size: [f32; 2], scale: f32, images: &Images, x: f32, y: f32) {
    if let Some(pm) = draw_frame(&snap.frame, size, scale, images) {
        sheet.draw_pixmap(x as i32, y as i32, pm.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        let mut pb = PathBuilder::new();
        if let Some(r) = Rect::from_xywh(x - 0.5, y - 0.5, pm.width() as f32 + 1.0, pm.height() as f32 + 1.0) {
            pb.push_rect(r);
        }
        if let Some(path) = pb.finish() {
            let stroke = tiny_skia::Stroke { width: 1.0, ..Default::default() };
            sheet.stroke_path(&path, &paint(Color::from_rgba8(255, 255, 255, 30)), &stroke, Transform::identity(), None);
        }
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decode base64 (a `data:` URL prefix is skipped).
pub fn unbase64(s: &str) -> Option<Vec<u8>> {
    let s = s.split_once("base64,").map(|(_, b)| b).unwrap_or(s);
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        } as u32;
        buf = buf << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}
