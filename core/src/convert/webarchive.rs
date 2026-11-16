//! Safari's saved pages (.webarchive): a binary property list with the page (`WebMainResource`)
//! and what it loaded (`WebSubresources`: images, scripts, styles), each with its URL and bytes.
//! A port of `WebArchive.swift`, with a small reader for binary property lists (`bplist00`).

use super::webpage::PageImage;
use super::{fail, images, Result};

pub struct Page {
    pub html: String,
    pub url: Option<url::Url>,
    pub images: Vec<PageImage>,
}

pub fn is_webarchive(d: &[u8]) -> bool {
    d.starts_with(b"bplist00") && d.windows(15).any(|w| w == b"WebMainResource")
}

pub fn read(data: &[u8]) -> Result<Page> {
    let not = || fail("Not a Safari web archive.");
    let plist = Plist::parse(data).ok_or_else(not)?;
    let main = plist.get("WebMainResource").ok_or_else(not)?;
    let body = main.get("WebResourceData").and_then(Value::data).ok_or_else(not)?;
    let enc = main.get("WebResourceTextEncodingName").and_then(Value::str).unwrap_or("utf-8").to_lowercase();
    let html = match std::str::from_utf8(body) {
        Ok(s) if enc.contains("utf") => s.to_string(),
        _ => body.iter().map(|&b| b as char).collect(), // ISO Latin-1
    };
    let url = main.get("WebResourceURL").and_then(Value::str).and_then(|u| url::Url::parse(u).ok());
    let mut imgs = Vec::new();
    if let Some(Value::Array(subs)) = plist.get("WebSubresources") {
        for r in subs {
            let (Some(u), Some(d)) = (r.get("WebResourceURL").and_then(Value::str), r.get("WebResourceData").and_then(Value::data))
            else {
                continue;
            };
            let mime = r.get("WebResourceMIMEType").and_then(Value::str).unwrap_or("");
            if mime.starts_with("image/") || images::is_image(d) {
                imgs.push(PageImage { urls: vec![u.to_string()], data: d.to_vec() });
            }
        }
    }
    Ok(Page { html, url, images: imgs })
}

#[derive(Debug)]
enum Value {
    Dict(Vec<(String, Value)>),
    Array(Vec<Value>),
    Data(Vec<u8>),
    Str(String),
    Other,
}

impl Value {
    fn get(&self, k: &str) -> Option<&Value> {
        match self {
            Value::Dict(v) => v.iter().find(|(n, _)| n == k).map(|(_, x)| x),
            _ => None,
        }
    }
    fn data(&self) -> Option<&[u8]> {
        match self {
            Value::Data(d) => Some(d),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// A binary property list (Apple's `bplist00`): the object table, read from the trailer.
struct Plist<'a> {
    d: &'a [u8],
    offsets: Vec<usize>,
    ref_size: usize,
}

impl Plist<'_> {
    fn parse(d: &[u8]) -> Option<Value> {
        if d.len() < 40 || !d.starts_with(b"bplist00") {
            return None;
        }
        let t = &d[d.len() - 32..];
        let off_size = t[6] as usize;
        let ref_size = t[7] as usize;
        let num = be(&t[8..16]) as usize;
        let top = be(&t[16..24]) as usize;
        let table = be(&t[24..32]) as usize;
        if off_size == 0 || off_size > 8 || ref_size == 0 || ref_size > 8 || top >= num {
            return None;
        }
        let end = table.checked_add(num.checked_mul(off_size)?)?;
        if end > d.len() {
            return None;
        }
        let offsets = (0..num).map(|i| be(&d[table + i * off_size..table + (i + 1) * off_size]) as usize).collect();
        let p = Plist { d, offsets, ref_size };
        p.object(top, 0)
    }

    /// (length, where the content starts) for an object whose marker is at `at`.
    fn length(&self, at: usize) -> Option<(usize, usize)> {
        let low = (self.d.get(at)? & 0x0F) as usize;
        if low != 0x0F {
            return Some((low, at + 1));
        }
        let m = *self.d.get(at + 1)?;
        if m >> 4 != 0x1 {
            return None;
        }
        let n = 1usize << (m & 0x0F);
        let bytes = self.d.get(at + 2..at + 2 + n)?;
        Some((be(bytes) as usize, at + 2 + n))
    }

    fn object(&self, idx: usize, depth: usize) -> Option<Value> {
        if depth > 64 {
            return None;
        }
        let at = *self.offsets.get(idx)?;
        let marker = *self.d.get(at)?;
        Some(match marker >> 4 {
            0x4 => {
                let (n, s) = self.length(at)?;
                Value::Data(self.d.get(s..s.checked_add(n)?)?.to_vec())
            }
            0x5 => {
                let (n, s) = self.length(at)?;
                Value::Str(self.d.get(s..s.checked_add(n)?)?.iter().map(|&b| b as char).collect())
            }
            0x6 => {
                let (n, s) = self.length(at)?;
                let b = self.d.get(s..s.checked_add(n.checked_mul(2)?)?)?;
                let units: Vec<u16> = b.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                Value::Str(String::from_utf16_lossy(&units))
            }
            0xA => {
                let (n, s) = self.length(at)?;
                let mut v = Vec::with_capacity(n.min(4096));
                for i in 0..n {
                    let r = self.reference(s + i * self.ref_size)?;
                    v.push(self.object(r, depth + 1)?);
                }
                Value::Array(v)
            }
            0xD => {
                let (n, s) = self.length(at)?;
                let mut v = Vec::with_capacity(n.min(4096));
                for i in 0..n {
                    let k = self.reference(s + i * self.ref_size)?;
                    let o = self.reference(s + (n + i) * self.ref_size)?;
                    let key = match self.object(k, depth + 1)? {
                        Value::Str(s) => s,
                        _ => continue,
                    };
                    v.push((key, self.object(o, depth + 1)?));
                }
                Value::Dict(v)
            }
            _ => Value::Other,
        })
    }

    fn reference(&self, at: usize) -> Option<usize> {
        Some(be(self.d.get(at..at + self.ref_size)?) as usize)
    }
}

fn be(b: &[u8]) -> u64 {
    b.iter().fold(0u64, |a, &x| (a << 8) | x as u64)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal binary property list writer for the tests: `{WebMainResource: {...}, WebSubresources: [...]}`.
    pub(crate) fn archive(html: &str, url: &str, subs: &[(&str, &str, &[u8])]) -> Vec<u8> {
        enum O {
            S(String),
            D(Vec<u8>),
            Dict(Vec<(usize, usize)>),
            Arr(Vec<usize>),
        }
        let mut objs: Vec<O> = Vec::new();
        let mut add = |o: O| {
            objs.push(o);
            objs.len() - 1
        };
        let k_data = add(O::S("WebResourceData".into()));
        let k_url = add(O::S("WebResourceURL".into()));
        let k_mime = add(O::S("WebResourceMIMEType".into()));
        let k_enc = add(O::S("WebResourceTextEncodingName".into()));
        let k_main = add(O::S("WebMainResource".into()));
        let k_subs = add(O::S("WebSubresources".into()));
        let h = add(O::D(html.as_bytes().to_vec()));
        let u = add(O::S(url.into()));
        let m = add(O::S("text/html".into()));
        let e = add(O::S("UTF-8".into()));
        let main = add(O::Dict(vec![(k_data, h), (k_url, u), (k_mime, m), (k_enc, e)]));
        let mut sub_ids = Vec::new();
        for (su, mime, d) in subs {
            let a = add(O::S(su.to_string()));
            let b = add(O::S(mime.to_string()));
            let c = add(O::D(d.to_vec()));
            sub_ids.push(add(O::Dict(vec![(k_url, a), (k_mime, b), (k_data, c)])));
        }
        let arr = add(O::Arr(sub_ids));
        let top = add(O::Dict(vec![(k_main, main), (k_subs, arr)]));
        let mut out = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        let len = |out: &mut Vec<u8>, kind: u8, n: usize| {
            if n < 15 {
                out.push(kind << 4 | n as u8);
            } else {
                out.push(kind << 4 | 0xF);
                out.push(0x13);
                out.extend_from_slice(&(n as u64).to_be_bytes());
            }
        };
        for o in &objs {
            offsets.push(out.len());
            match o {
                O::S(s) => {
                    len(&mut out, 0x5, s.len());
                    out.extend_from_slice(s.as_bytes());
                }
                O::D(d) => {
                    len(&mut out, 0x4, d.len());
                    out.extend_from_slice(d);
                }
                O::Dict(kv) => {
                    len(&mut out, 0xD, kv.len());
                    for (k, _) in kv {
                        out.extend_from_slice(&(*k as u16).to_be_bytes());
                    }
                    for (_, v) in kv {
                        out.extend_from_slice(&(*v as u16).to_be_bytes());
                    }
                }
                O::Arr(v) => {
                    len(&mut out, 0xA, v.len());
                    for x in v {
                        out.extend_from_slice(&(*x as u16).to_be_bytes());
                    }
                }
            }
        }
        let table = out.len();
        for o in &offsets {
            out.extend_from_slice(&(*o as u64).to_be_bytes());
        }
        let mut trailer = vec![0u8; 6];
        trailer.push(8);
        trailer.push(2);
        trailer.extend_from_slice(&(objs.len() as u64).to_be_bytes());
        trailer.extend_from_slice(&(top as u64).to_be_bytes());
        trailer.extend_from_slice(&(table as u64).to_be_bytes());
        out.extend_from_slice(&trailer);
        out
    }

    #[test]
    fn reads_a_web_archive() {
        let png = [0x89u8, 0x50, 0x4E, 0x47, 1, 2, 3];
        let d = archive(
            "<html><body>é</body></html>",
            "https://www.example.org/article",
            &[("https://www.example.org/a.png", "image/png", &png), ("https://www.example.org/s.js", "text/javascript", b"x()")],
        );
        assert!(is_webarchive(&d));
        let p = read(&d).unwrap();
        assert_eq!(p.html, "<html><body>é</body></html>");
        assert_eq!(p.url.unwrap().as_str(), "https://www.example.org/article");
        assert_eq!(p.images.len(), 1);
        assert_eq!(p.images[0].urls, vec!["https://www.example.org/a.png"]);
        assert!(read(b"bplist00garbage").is_err());
    }
}
