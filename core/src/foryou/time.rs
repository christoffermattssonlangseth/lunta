//! Dates as New for You needs them, without a time-zone database: instants are milliseconds
//! since 1970 (UTC); the local day is worked out from the offset the app passes in (minutes east
//! of UTC, e.g. 60 in Stockholm in winter). Dates in foryou.json are written as Swift's
//! JSONEncoder writes `.iso8601` ("2026-11-19T07:00:00Z", no fractions: Swift can't read them).

pub const DAY_MS: i64 = 86_400_000;
pub const HOUR_MS: i64 = 3_600_000;

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's civil_from_days).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day) → days since 1970-01-01.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "2026-11-19T09:30:00Z".
pub fn iso8601(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

/// Reads "2026-11-19T09:30:00Z", "…T09:30:00.123Z" or "…T10:30:00+01:00".
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
        o if o.len() == 5 && (o.starts_with('+') || o.starts_with('-')) => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            sign * (o[1..3].parse::<i64>().ok()? * 3600 + o[3..5].parse::<i64>().ok()? * 60)
        }
        _ => return None,
    };
    Some((days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se - offset) * 1000 + ms)
}

/// "2026-11-19": the UTC date of an instant (what OpenAlex's from_publication_date takes).
pub fn utc_date(ms: i64) -> String {
    let (y, m, d) = civil(ms.div_euclid(DAY_MS));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Midnight of the local day an instant falls on.
pub fn start_of_day(ms: i64, tz_min: i32) -> i64 {
    let off = tz_min as i64 * 60_000;
    (ms + off).div_euclid(DAY_MS) * DAY_MS - off
}

/// The local day number (days since 1970-01-01 in local time).
pub fn local_day(ms: i64, tz_min: i32) -> i64 {
    (ms + tz_min as i64 * 60_000).div_euclid(DAY_MS)
}

/// A paper's publication date ("2026-11-06") as local midnight; None when only the month or
/// year is known.
pub fn published(s: &str, tz_min: i32) -> Option<i64> {
    if s.len() < 10 {
        return None;
    }
    let num = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d) * DAY_MS - tz_min as i64 * 60_000)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "Published 6 Nov (7 days ago)", or the date as given when it isn't a full one
/// (WatchCenter.publishedLabel).
pub fn published_label(s: &str, now: i64, tz_min: i32) -> Option<String> {
    let Some(d) = published(s, tz_min) else {
        return (!s.is_empty()).then(|| format!("Published {s}"));
    };
    let days = local_day(now, tz_min) - local_day(d, tz_min);
    let ago = match days {
        i64::MIN..=0 => "today".to_string(),
        1 => "yesterday".to_string(),
        n => format!("{n} days ago"),
    };
    let (_, m, day) = civil(local_day(d, tz_min));
    Some(format!("Published {day} {} ({ago})", MONTHS[(m - 1) as usize]))
}

/// serde for Swift's `.iso8601` dates.
pub mod iso {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(ms: &i64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::iso8601(*ms))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        from_value(&v).ok_or_else(|| serde::de::Error::custom("not an ISO 8601 date"))
    }

    /// An ISO string, or a number of seconds since 2001 (Swift's default date encoding).
    pub(crate) fn from_value(v: &serde_json::Value) -> Option<i64> {
        match v {
            serde_json::Value::String(s) => super::parse_iso8601(s),
            serde_json::Value::Number(n) => n.as_f64().map(|x| ((x + 978_307_200.0) * 1000.0) as i64),
            _ => None,
        }
    }

    pub mod opt {
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(ms: &Option<i64>, s: S) -> Result<S::Ok, S::Error> {
            match ms {
                Some(ms) => s.serialize_str(&super::super::iso8601(*ms)),
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
            let v = serde_json::Value::deserialize(d)?;
            Ok(super::from_value(&v))
        }
    }
}
