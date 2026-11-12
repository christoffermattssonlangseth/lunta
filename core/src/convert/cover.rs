//! Generated cover (same design as the Swift `Cover.swift` and the Python tool): journal band,
//! year, title, accent rule, authors, DOI. 1200 × 1800 JPEG.
//!
//! Text is drawn with system fonts found at runtime (no fonts are bundled): Helvetica Neue and
//! Georgia Bold on macOS (what the Swift code uses), DejaVu / Liberation / Noto on Linux,
//! Arial / Georgia on Windows, Roboto / Noto on Android. Set `P2E_FONT_DIR` to add a folder to
//! search. Without any usable font there is no cover (the EPUB is still valid).

use super::PaperMeta;
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use image::{Rgb, RgbImage};
use std::path::PathBuf;

pub const WIDTH: u32 = 1200;
pub const HEIGHT: u32 = 1800;
const TEXT_W: f32 = 1020.0;
const LEFT: f32 = 90.0;

fn font_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("P2E_FONT_DIR") {
        v.push(d.into());
    }
    for d in [
        "/System/Library/Fonts",
        "/System/Library/Fonts/Supplemental",
        "/Library/Fonts",
        "/usr/share/fonts/truetype/dejavu",
        "/usr/share/fonts/dejavu",
        "/usr/share/fonts/TTF",
        "/usr/share/fonts/truetype/liberation",
        "/usr/share/fonts/liberation",
        "/usr/share/fonts/truetype/noto",
        "/usr/share/fonts/noto",
        "C:\\Windows\\Fonts",
        "/system/fonts",
    ] {
        v.push(d.into());
    }
    if let Ok(w) = std::env::var("WINDIR") {
        v.push(PathBuf::from(w).join("Fonts"));
    }
    v
}

/// Loads the face with PostScript name `ps` from one of `files` (a .ttc is searched face by face);
/// fallback files (no PostScript name match needed) are tried after.
fn load(ps: &str, files: &[&str], fallbacks: &[&str]) -> Option<FontVec> {
    let dirs = font_dirs();
    for f in files {
        for d in &dirs {
            let Ok(data) = std::fs::read(d.join(f)) else { continue };
            let n = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
            for i in 0..n {
                let Ok(face) = ttf_parser::Face::parse(&data, i) else { continue };
                let name = face
                    .names()
                    .into_iter()
                    .filter(|r| r.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
                    // Unicode / Windows names decode as UTF-16; Macintosh ones are (ASCII) Mac Roman
                    .map(|r| r.to_string().unwrap_or_else(|| r.name.iter().map(|&b| b as char).collect()))
                    .any(|n| n == ps);
                if name {
                    if let Ok(font) = FontVec::try_from_vec_and_index(data.clone(), i) {
                        return Some(font);
                    }
                }
            }
        }
    }
    for f in fallbacks {
        for d in &dirs {
            if let Ok(data) = std::fs::read(d.join(f)) {
                if let Ok(font) = FontVec::try_from_vec_and_index(data, 0) {
                    return Some(font);
                }
            }
        }
    }
    None
}

struct Fonts {
    medium: FontVec,
    regular: FontVec,
    serif_bold: FontVec,
}

fn fonts() -> Option<Fonts> {
    let sans = ["DejaVuSans.ttf", "LiberationSans-Regular.ttf", "arial.ttf", "Arial.ttf", "Roboto-Regular.ttf", "NotoSans-Regular.ttf"];
    let regular = load("HelveticaNeue", &["HelveticaNeue.ttc"], &sans)?;
    let medium = load(
        "HelveticaNeue-Medium",
        &["HelveticaNeue.ttc"],
        &["Roboto-Medium.ttf", "DejaVuSans.ttf", "LiberationSans-Regular.ttf", "arial.ttf", "Arial.ttf", "NotoSans-Regular.ttf"],
    )?;
    let serif_bold = load(
        "Georgia-Bold",
        &["Georgia Bold.ttf", "georgiab.ttf"],
        &["DejaVuSerif-Bold.ttf", "LiberationSerif-Bold.ttf", "NotoSerif-Bold.ttf", "georgiab.ttf"],
    )?;
    Some(Fonts { medium, regular, serif_bold })
}

/// Font size in CoreText terms (pixels per em) → ab_glyph scale.
fn scale(f: &FontVec, size: f32) -> PxScale {
    let upem = f.units_per_em().unwrap_or(1000.0);
    PxScale::from(size * f.height_unscaled() / upem)
}

/// Word-wraps `s` to `TEXT_W` pixels; words wider than a line are broken by character.
fn wrap(f: &FontVec, size: f32, s: &str) -> Vec<String> {
    let sf = f.as_scaled(scale(f, size));
    let width = |t: &str| -> f32 {
        let mut w = 0.0;
        let mut prev = None;
        for c in t.chars() {
            let g = sf.glyph_id(c);
            if let Some(p) = prev {
                w += sf.kern(p, g);
            }
            w += sf.h_advance(g);
            prev = Some(g);
        }
        w
    };
    let mut lines: Vec<String> = Vec::new();
    for para in s.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if width(&cand) <= TEXT_W {
                cur = cand;
                continue;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            if width(word) <= TEXT_W {
                cur = word.to_string();
            } else {
                for c in word.chars() {
                    let mut t = cur.clone();
                    t.push(c);
                    if width(&t) > TEXT_W && !cur.is_empty() {
                        lines.push(std::mem::take(&mut cur));
                        cur.push(c);
                    } else {
                        cur = t;
                    }
                }
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
    }
    lines
}

/// CoreText-like line metrics: line height = ascent + descent + gap, plus 0.22 em between lines
/// (the Swift code's `lineSpacingAdjustment`).
fn metrics(f: &FontVec, size: f32) -> (f32, f32) {
    let sf = f.as_scaled(scale(f, size));
    let line = sf.ascent() - sf.descent() + sf.line_gap();
    (sf.ascent(), line + size * 0.22)
}

fn text_height(f: &FontVec, size: f32, s: &str) -> f32 {
    let n = wrap(f, size, s).len() as f32;
    let (_, step) = metrics(f, size);
    n * step - size * 0.22
}

/// Draws wrapped text whose top edge is `top` px from the top of the image; returns the height used.
fn draw(img: &mut RgbImage, f: &FontVec, size: f32, color: u32, s: &str, top: f32, max_height: f32) -> f32 {
    let sc = scale(f, size);
    let sf = f.as_scaled(sc);
    let (ascent, step) = metrics(f, size);
    let line_h = step - size * 0.22;
    let rgb = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
    let mut used = 0.0f32;
    for (i, line) in wrap(f, size, s).iter().enumerate() {
        let y0 = i as f32 * step;
        if y0 + line_h > max_height + 0.5 {
            break;
        }
        used = y0 + line_h;
        let baseline = top + y0 + ascent;
        let mut x = LEFT;
        let mut prev = None;
        for c in line.chars() {
            let gid = sf.glyph_id(c);
            if let Some(p) = prev {
                x += sf.kern(p, gid);
            }
            let g = gid.with_scale_and_position(sc, ab_glyph::point(x, baseline));
            x += sf.h_advance(gid);
            prev = Some(gid);
            if let Some(o) = f.outline_glyph(g) {
                let b = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let px = b.min.x as i32 + gx as i32;
                    let py = b.min.y as i32 + gy as i32;
                    if px < 0 || py < 0 || px >= WIDTH as i32 || py >= HEIGHT as i32 {
                        return;
                    }
                    let p = img.get_pixel_mut(px as u32, py as u32);
                    let a = cov.clamp(0.0, 1.0);
                    for k in 0..3 {
                        p.0[k] = (p.0[k] as f32 * (1.0 - a) + rgb[k] as f32 * a).round() as u8;
                    }
                });
            }
        }
    }
    used.min(max_height).ceil()
}

fn fill(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, color: u32) {
    let c = Rgb([(color >> 16) as u8, (color >> 8) as u8, color as u8]);
    for y in y0..y1.min(HEIGHT) {
        for x in x0..x1.min(WIDTH) {
            img.put_pixel(x, y, c);
        }
    }
}

/// The cover as JPEG, or None when no font is available.
pub fn make(meta: &PaperMeta) -> Option<Vec<u8>> {
    let fonts = fonts()?;
    let mut img = RgbImage::from_pixel(WIDTH, HEIGHT, Rgb([0xf6, 0xf3, 0xec]));
    fill(&mut img, 0, 0, WIDTH, 360, 0x1f4e5f);

    let journal = if meta.journal.is_empty() { "Scientific paper".to_string() } else { meta.journal.clone() };
    let journal: String = journal.to_uppercase().chars().take(48).collect();
    draw(&mut img, &fonts.medium, 38.0, 0xe8eef0, &journal, 150.0, 50.0);
    let year: String = meta.date.chars().take(4).collect();
    if !year.is_empty() {
        draw(&mut img, &fonts.regular, 34.0, 0xa9c3cc, &year, 215.0, 50.0);
    }

    // Title: shrink until it fits in ~820 px
    let title = if meta.title.is_empty() { "Untitled paper" } else { &meta.title };
    let mut size = 82.0;
    while size > 40.0 && text_height(&fonts.serif_bold, size, title) > 820.0 {
        size -= 6.0;
    }
    let th = draw(&mut img, &fonts.serif_bold, size, 0x1d1d1f, title, 460.0, 900.0);

    let mut y = 460.0 + th + 40.0;
    fill(&mut img, 90, (y - 3.0) as u32, 330, (y + 3.0) as u32, 0xc2703d);
    y += 50.0;
    if !meta.authors.is_empty() {
        let mut a = meta.authors.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        if meta.authors.len() > 8 {
            a += " et al.";
        }
        let h = HEIGHT as f32;
        draw(&mut img, &fonts.regular, 36.0, 0x3a3a3c, &a, y, (h - y - 180.0).max(60.0));
    }
    if !meta.doi.is_empty() {
        draw(&mut img, &fonts.regular, 28.0, 0x6e6e73, &format!("doi:{}", meta.doi), HEIGHT as f32 - 140.0, 50.0);
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88).encode_image(&img).ok()?;
    Some(out)
}

