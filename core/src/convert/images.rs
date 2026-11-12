//! Figure images: normalising (downscale / re-encode) and downloading the images a JATS
//! document references. A port of `Images.swift` (ImageIO → the `image` crate).

use super::net;
use super::resolver::AssetLocator;
use super::{slug, BookImage, Options};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Cursor;

pub struct Normalised {
    pub data: Vec<u8>,
    pub ext: &'static str,
    pub media_type: &'static str,
}

/// Number of colour components of a JPEG (from its SOF marker); 4 = CMYK/YCCK.
fn jpeg_components(d: &[u8]) -> Option<u8> {
    let mut i = 2;
    while i + 9 < d.len() {
        if d[i] != 0xFF {
            return None;
        }
        let m = d[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
            return d.get(i + 9).copied();
        }
        i += 2 + len;
    }
    None
}

/// Downscales and re-encodes an image for the EPUB. PNG/GIF stay PNG (line art, formulas);
/// everything else (TIFF, JPEG, …) becomes JPEG. None if the image can't be read.
pub fn normalise(data: &[u8], max_pixels: u32) -> Option<Normalised> {
    let format = image::guess_format(data).ok()?;
    let reader = ImageReader::with_format(Cursor::new(data), format);
    let mut decoder = reader.into_decoder().ok()?;
    let (w, h) = decoder.dimensions();
    let orientation = decoder.orientation().ok();
    let is_jpeg = format == ImageFormat::Jpeg;
    let keep_png = format == ImageFormat::Png || format == ImageFormat::Gif;

    // Small enough RGB JPEGs pass through untouched.
    if is_jpeg && w.max(h) <= max_pixels && jpeg_components(data) != Some(4) && data.len() < 1_500_000 {
        return Some(Normalised { data: data.to_vec(), ext: "jpg", media_type: "image/jpeg" });
    }
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    let (w, h) = (img.width(), img.height());
    let target = max_pixels.min(w.max(h).max(1));
    if w.max(h) > target {
        img = img.resize(target, target, image::imageops::FilterType::Lanczos3);
    }
    let mut out = Vec::new();
    if keep_png {
        let rgba = img.to_rgba8();
        let opaque = rgba.pixels().all(|p| p[3] == 255);
        let img = if opaque { DynamicImage::ImageRgb8(img.to_rgb8()) } else { DynamicImage::ImageRgba8(rgba) };
        // best compression: line art from arXiv/PMC compresses well, ImageIO's PNGs are small too
        let enc = image::codecs::png::PngEncoder::new_with_quality(
            &mut out,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Adaptive,
        );
        img.write_with_encoder(enc).ok()?;
        Some(Normalised { data: out, ext: "png", media_type: "image/png" })
    } else {
        // opaque: flatten onto white
        let rgba = img.to_rgba8();
        let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
        for (x, y, p) in rgba.enumerate_pixels() {
            let a = p[3] as u32;
            let c = |v: u8| ((v as u32 * a + 255 * (255 - a)) / 255) as u8;
            rgb.put_pixel(x, y, image::Rgb([c(p[0]), c(p[1]), c(p[2])]));
        }
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82);
        enc.encode_image(&rgb).ok()?;
        Some(Normalised { data: out, ext: "jpg", media_type: "image/jpeg" })
    }
}

pub fn is_image(d: &[u8]) -> bool {
    d.starts_with(&[0xFF, 0xD8, 0xFF])
        || d.starts_with(&[0x89, 0x50, 0x4E, 0x47])
        || d.starts_with(b"GIF8")
        || d.starts_with(&[0x49, 0x49, 0x2A, 0x00])
        || d.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
}

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "tif", "tiff", "bmp", "webp"];

fn last_path_component(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p)
}

pub fn ext(p: &str) -> String {
    let last = last_path_component(p);
    match last.rfind('.') {
        Some(i) if i > 0 => last[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

pub fn stem(p: &str) -> String {
    let last = last_path_component(p);
    let e = ext(last);
    if IMAGE_EXTS.contains(&e.as_str()) || ["eps", "pdf", "svg", "xml"].contains(&e.as_str()) {
        last[..last.len() - e.len() - 1].to_string()
    } else {
        last.to_string()
    }
}

/// Downloads and normalises the figure images a JATS document references.
/// Returns href → image (with a unique file name). Missing images are simply absent.
pub fn fetch_assets(hrefs: &[String], locator: &AssetLocator, o: &Options) -> HashMap<String, BookImage> {
    let unique: Vec<String> =
        hrefs.iter().filter(|h| !h.is_empty()).cloned().collect::<BTreeSet<_>>().into_iter().collect();
    if unique.is_empty() || matches!(locator, AssetLocator::None) {
        return HashMap::new();
    }
    o.log(&format!("Downloading {} figure image(s)", unique.len()));
    let fetched = net::par_map(&unique, 6, |h| download(h, locator).and_then(|d| normalise(&d, 1600)));
    let mut result = HashMap::new();
    let mut used = HashSet::new();
    for (href, norm) in unique.iter().zip(fetched) {
        let Some(n) = norm else { continue };
        let mut base = slug(&stem(href), 60);
        if base.is_empty() {
            base = "img".into();
        }
        let mut name = format!("{base}.{}", n.ext);
        let mut i = 2;
        while used.contains(&name) {
            name = format!("{base}-{i}.{}", n.ext);
            i += 1;
        }
        used.insert(name.clone());
        result.insert(href.clone(), BookImage { file_name: name, data: n.data, media_type: n.media_type.into() });
    }
    result
}

fn download(href: &str, locator: &AssetLocator) -> Option<Vec<u8>> {
    match locator {
        AssetLocator::None => None,
        AssetLocator::S3 { base, files } => {
            let name = last_path_component(href);
            let st = stem(href);
            // exact name first, then same stem with a web-friendly extension preferred
            let mut options: Vec<&String> = files.iter().filter(|f| *f == name || *f == href).collect();
            let rank = |f: &str| match ext(f).as_str() {
                "jpg" | "jpeg" => 0,
                "png" => 1,
                "gif" => 2,
                "tif" | "tiff" => 3,
                _ => 9,
            };
            let mut same: Vec<&String> =
                files.iter().filter(|f| stem(f) == st && IMAGE_EXTS.contains(&ext(f).as_str())).collect();
            same.sort_by_key(|f| rank(f));
            options.extend(same);
            for f in options {
                if let Ok(d) = net::get(&format!("{base}/{}", net::encode_path(f)), None, 90.0) {
                    if is_image(&d) {
                        return Some(d);
                    }
                }
            }
            None
        }
        AssetLocator::Patterns(pats) => {
            let enc = net::encode_path(href);
            let st = net::encode_path(&stem(href));
            for p in pats {
                let u = p.replace("{href}", &enc).replace("{stem}", &st);
                if let Ok(d) = net::get(&u, None, 60.0) {
                    if is_image(&d) {
                        return Some(d);
                    }
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_and_normalise() {
        assert_eq!(stem("fig/41586_2020_2012_Fig1_HTML.jpg"), "41586_2020_2012_Fig1_HTML");
        assert_eq!(stem("elife-00001-fig1"), "elife-00001-fig1");
        assert_eq!(stem("nihms-1.tif"), "nihms-1");
        // a 2000x10 PNG is scaled to 1600 wide and stays PNG
        let img = image::RgbImage::from_pixel(2000, 10, image::Rgb([10, 20, 30]));
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let n = normalise(&png, 1600).unwrap();
        assert_eq!(n.ext, "png");
        let back = image::load_from_memory(&n.data).unwrap();
        assert_eq!(back.width(), 1600);
    }
}
