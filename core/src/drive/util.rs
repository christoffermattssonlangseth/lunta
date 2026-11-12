//! Small helpers: RFC 3339 times, MD5, hex, atomic writes.

use crate::model::{CoreError, Result};
use md5::{Digest, Md5};
use std::fs;
use std::io::Read;
use std::path::Path;

pub fn md5_file(p: &Path) -> Result<String> {
    let mut f = fs::File::open(p)?;
    let mut h = Md5::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

pub fn md5_bytes(b: &[u8]) -> String {
    hex(&Md5::digest(b))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// (size, modified time in ms since the epoch) of a file.
pub fn file_sig(p: &Path) -> Option<(u64, i64)> {
    let m = fs::metadata(p).ok()?;
    if !m.is_file() {
        return None;
    }
    let mt = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64;
    Some((m.len(), mt))
}

pub fn set_mtime(p: &Path, ms: i64) {
    if ms <= 0 {
        return;
    }
    if let Ok(f) = fs::OpenOptions::new().write(true).open(p) {
        let _ = f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64));
    }
}

/// Writes `data` to `p` through a temp file in the same folder, so readers never see a half file.
pub fn write_atomic(p: &Path, data: &[u8]) -> Result<()> {
    if let Some(d) = p.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = p.with_file_name(format!(".{}.tmp", p.file_name().unwrap_or_default().to_string_lossy()));
    fs::write(&tmp, data)?;
    fs::rename(&tmp, p)?;
    Ok(())
}

// ---- RFC 3339 <-> ms (UTC only, which is all Drive returns) ----

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let mut i = 19;
    let mut ms = 0;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let frac = &s[start..i];
        let three = format!("{:0<3}", &frac[..frac.len().min(3)]);
        ms = three.parse::<i64>().ok()?;
    }
    let mut off = 0;
    match b.get(i) {
        Some(b'Z') | None => {}
        Some(&c) if c == b'+' || c == b'-' => {
            let oh = s.get(i + 1..i + 3)?.parse::<i64>().ok()?;
            let om = s.get(i + 4..i + 6)?.parse::<i64>().ok()?;
            off = (oh * 60 + om) * 60 * if c == b'+' { 1 } else { -1 };
        }
        _ => return None,
    }
    Some(((days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se - off) * 1000) + ms)
}

pub fn format_rfc3339(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (y, m, d) = civil_from_days(secs.div_euclid(86400));
    let r = secs.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", r / 3600, r % 3600 / 60, r % 60, ms.rem_euclid(1000))
}

/// Percent-encodes everything but unreserved characters (query values, form bodies).
pub fn urlenc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

pub fn urldec(s: &str) -> String {
    let b = s.as_bytes();
    let mut o = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    o.push(v);
                    i += 3;
                    continue;
                }
                o.push(b'%');
                i += 1;
            }
            b'+' => {
                o.push(b' ');
                i += 1;
            }
            c => {
                o.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&o).to_string()
}

pub fn net<E: std::fmt::Display>(e: E) -> CoreError {
    CoreError::Network(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn time_roundtrip() {
        let ms = parse_rfc3339("2026-03-04T05:06:07.089Z").unwrap();
        assert_eq!(format_rfc3339(ms), "2026-03-04T05:06:07.089Z");
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(parse_rfc3339("2024-02-29T00:00:00.5Z").unwrap() % 1000, 500);
    }
    #[test]
    fn url_codec() {
        assert_eq!(urldec(&urlenc("a b&c=d/é")), "a b&c=d/é");
        assert_eq!(urldec("100%"), "100%");
    }
}
