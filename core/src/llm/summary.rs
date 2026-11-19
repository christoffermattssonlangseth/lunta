//! summary.json in a paper's folder: the Summary card's text, kept with the paper so it syncs
//! with the library and is there at once next time, on every device. The same file the Apple apps
//! write (PaperSummary in apple/Shared/Summary.swift, JSONEncoder with ISO 8601 dates, no
//! fractional seconds), so a summary made on any device is reused on the others.

use crate::model::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE_NAME: &str = "summary.json";

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PaperSummary {
    pub text: String,
    /// "apple-on-device", "qwen3-4b", "qwen3-1.7b"
    pub model: String,
    /// ms since 1970
    pub created: i64,
    /// prompt::source_hash of what it was made from
    pub source: String,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    created: String,
    model: String,
    source: String,
    text: String,
}

/// Days since 1970-01-01 → (y, m, d).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "2026-11-19T09:30:00Z", as Swift's JSONEncoder writes .iso8601 dates.
pub fn iso8601(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

/// Reads "2026-11-19T09:30:00Z", "…T09:30:00.123Z" or "…T10:30:00+01:00" (ms since 1970).
pub fn parse_iso8601(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d, h, mi, se) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if s.as_bytes().get(10) != Some(&b'T') {
        return None;
    }
    let mut rest = &s[19..];
    let mut ms = 0;
    if let Some(r) = rest.strip_prefix('.') {
        let digits: String = r.chars().take_while(char::is_ascii_digit).collect();
        ms = format!("{:0<3}", &digits[..digits.len().min(3)]).parse().unwrap_or(0);
        rest = &r[digits.len()..];
    }
    let offset = match rest {
        "Z" | "" => 0,
        o if o.len() == 6 && (o.starts_with('+') || o.starts_with('-')) => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            sign * (o[1..3].parse::<i64>().ok()? * 3600 + o[4..6].parse::<i64>().ok()? * 60)
        }
        _ => return None,
    };
    Some((days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se - offset) * 1000 + ms)
}

/// The summary kept in a paper's folder (None if there is none, or it can't be read).
pub fn read(paper_dir: &Path) -> Option<PaperSummary> {
    let text = std::fs::read_to_string(paper_dir.join(FILE_NAME)).ok()?;
    let d: OnDisk = serde_json::from_str(&text).ok()?;
    Some(PaperSummary { text: d.text, model: d.model, created: parse_iso8601(&d.created).unwrap_or(0), source: d.source })
}

/// Swift's .prettyPrinted + .sortedKeys layout (keys sorted, two-space indent, " : ").
fn encode(s: &PaperSummary) -> String {
    let q = |v: &str| serde_json::to_string(v).unwrap_or_default();
    format!(
        "{{\n  \"created\" : {},\n  \"model\" : {},\n  \"source\" : {},\n  \"text\" : {}\n}}",
        q(&iso8601(s.created)),
        q(&s.model),
        q(&s.source),
        q(&s.text)
    )
}

pub fn write(paper_dir: &Path, s: &PaperSummary) -> Result<()> {
    std::fs::create_dir_all(paper_dir)?;
    let tmp = paper_dir.join(format!(".{FILE_NAME}.tmp"));
    std::fs::write(&tmp, encode(s))?;
    std::fs::rename(&tmp, paper_dir.join(FILE_NAME)).map_err(|e| CoreError::Io(e.to_string()))
}

pub fn remove(paper_dir: &Path) -> Result<()> {
    match std::fs::remove_file(paper_dir.join(FILE_NAME)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}
