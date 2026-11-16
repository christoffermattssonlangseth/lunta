//! Highlights made by quoting: finds a quoted passage in a paper's reading view and anchors it
//! the way the reader does (apple/Shared/Highlights.swift, Android's Reader.kt share the format):
//! `location` "epub", `position` the passage's start as a UTF-16 offset into the text of the
//! combined reading view's <body> (all its text nodes, as the page's DOM has them), and `quote`,
//! `prefix`, `suffix` exactly as that text has them, so the reader finds them with indexOf.
//! Papers with no reading view are anchored in their full text with `location` "pdf"; the PDF
//! view finds a highlight by its quote alone.
//!
//! Also: filtering highlights across the library (`find_annotations`).

use crate::model::*;

/// Text of the <body> of an XHTML document: every text node in document order (script and style
/// left out), entities decoded — what the reader's `textNodes()` concatenates.
pub fn dom_text(xhtml: &str) -> String {
    let start = match xhtml.find("<body") {
        Some(i) => xhtml[i..].find('>').map(|j| i + j + 1).unwrap_or(xhtml.len()),
        None => 0,
    };
    let end = xhtml.rfind("</body>").filter(|e| *e >= start).unwrap_or(xhtml.len());
    let s = &xhtml[start..end];
    let mut out = String::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if !rest.starts_with('<') {
            let next = rest.find('<').unwrap_or(rest.len());
            decode_into(&rest[..next], &mut out);
            i += next;
            continue;
        }
        if let Some(r) = rest.strip_prefix("<!--") {
            i += 4 + r.find("-->").map(|k| k + 3).unwrap_or(r.len());
        } else if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let k = r.find("]]>").unwrap_or(r.len());
            out.push_str(&r[..k]);
            i += 9 + (k + 3).min(r.len());
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            i += rest.find('>').map(|k| k + 1).unwrap_or(rest.len());
        } else {
            // a tag: find its end, minding quoted attribute values
            let bytes = rest.as_bytes();
            let mut k = 1;
            let mut q: Option<u8> = None;
            while k < bytes.len() {
                let b = bytes[k];
                match q {
                    Some(c) if b == c => q = None,
                    Some(_) => {}
                    None if b == b'"' || b == b'\'' => q = Some(b),
                    None if b == b'>' => break,
                    None => {}
                }
                k += 1;
            }
            let tag = &rest[..(k + 1).min(rest.len())];
            i += tag.len();
            let name: String = tag[1..].chars().take_while(|c| c.is_alphanumeric() || *c == ':' || *c == '-').collect::<String>().to_lowercase();
            if (name == "script" || name == "style") && !tag.ends_with("/>") {
                let close = format!("</{name}");
                let r = &s[i..];
                let at = r.to_lowercase().find(&close).unwrap_or(r.len());
                i += at;
                i += s[i..].find('>').map(|k| k + 1).unwrap_or(s.len() - i);
            }
        }
    }
    out
}

fn decode_into(s: &str, out: &mut String) {
    let mut rest = s;
    while let Some(a) = rest.find('&') {
        out.push_str(&rest[..a]);
        let r = &rest[a..];
        let end = r[1..].find(|c: char| c == ';' || c == '&' || c == '<' || c.is_whitespace()).map(|k| k + 1);
        if let Some(e) = end.filter(|e| r.as_bytes().get(*e) == Some(&b';') && *e <= 12) {
            let name = &r[1..e];
            let ch = if let Some(n) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(n, 16).ok().and_then(char::from_u32)
            } else if let Some(n) = name.strip_prefix('#') {
                n.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                named_entity(name)
            };
            if let Some(c) = ch {
                out.push(c);
                rest = &r[e + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &r[1..];
    }
    out.push_str(rest);
}

fn named_entity(n: &str) -> Option<char> {
    Some(match n {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "thinsp" => '\u{2009}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "ndash" => '–',
        "mdash" => '—',
        "minus" => '−',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "times" => '×',
        "plusmn" => '±',
        "deg" => '°',
        "micro" => 'µ',
        "middot" => '·',
        "le" => '≤',
        "ge" => '≥',
        "shy" => '\u{ad}',
        "zwsp" => '\u{200b}',
        _ => return None,
    })
}

/// Folded text for matching, with each folded char's byte range in the original.
struct Folded {
    chars: Vec<char>,
    map: Vec<(usize, usize)>,
}

fn is_space(c: char) -> bool {
    c.is_whitespace()
}

/// `dense`: whitespace left out entirely; otherwise runs of whitespace become one space.
/// Case, curly quotes, dashes, µ and ligatures are folded; soft hyphens and zero-width spaces dropped.
fn fold(s: &str, dense: bool) -> Folded {
    let mut f = Folded { chars: Vec::with_capacity(s.len()), map: Vec::with_capacity(s.len()) };
    for (i, c) in s.char_indices() {
        let r = (i, i + c.len_utf8());
        if c == '\u{ad}' || c == '\u{200b}' || c == '\u{200c}' || c == '\u{200d}' || c == '\u{feff}' {
            continue;
        }
        if is_space(c) {
            if !dense && f.chars.last() != Some(&' ') {
                f.chars.push(' ');
                f.map.push(r);
            }
            continue;
        }
        let push = |f: &mut Folded, x: char| {
            f.chars.push(x);
            f.map.push(r);
        };
        match c {
            '‘' | '’' | '‚' | '′' | '`' => push(&mut f, '\''),
            '“' | '”' | '„' | '″' => push(&mut f, '"'),
            '\u{2010}'..='\u{2015}' | '−' => push(&mut f, '-'),
            'µ' => push(&mut f, 'μ'),
            'ﬀ' => "ff".chars().for_each(|x| push(&mut f, x)),
            'ﬁ' => "fi".chars().for_each(|x| push(&mut f, x)),
            'ﬂ' => "fl".chars().for_each(|x| push(&mut f, x)),
            'ﬃ' => "ffi".chars().for_each(|x| push(&mut f, x)),
            'ﬄ' => "ffl".chars().for_each(|x| push(&mut f, x)),
            _ => c.to_lowercase().for_each(|x| push(&mut f, x)),
        }
    }
    f
}

fn trim_spaces(v: &[char]) -> &[char] {
    let a = v.iter().position(|c| *c != ' ').unwrap_or(v.len());
    let b = v.iter().rposition(|c| *c != ' ').map(|b| b + 1).unwrap_or(a);
    &v[a..b.max(a)]
}

fn find_all(hay: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return vec![];
    }
    let first = needle[0];
    (0..=hay.len() - needle.len()).filter(|&i| hay[i] == first && hay[i..i + needle.len()] == *needle).collect()
}

/// Where a quote is in `text`, as a byte range of `text`.
pub fn locate(text: &str, quote: &str, before: Option<&str>) -> Result<(usize, usize)> {
    let dense_q = fold(quote, true).chars;
    if dense_q.len() < 3 {
        return Err(CoreError::Invalid("the quote is too short: quote at least a few words".into()));
    }
    let dense_text = fold(text, true);
    let before_d = before.map(|b| fold(b, true).chars).filter(|b| !b.is_empty());
    let squashed = fold(text, false);
    for dense in [false, true] {
        let hay = if dense { &dense_text } else { &squashed };
        let q = fold(quote, dense).chars;
        let needle = trim_spaces(&q);
        let hits: Vec<(usize, usize)> =
            find_all(&hay.chars, needle).into_iter().map(|i| (hay.map[i].0, hay.map[i + needle.len() - 1].1)).collect();
        if hits.is_empty() {
            continue;
        }
        let chosen: Vec<(usize, usize)> = match &before_d {
            Some(b) if hits.len() > 1 => hits
                .iter()
                .copied()
                .filter(|(s, _)| {
                    let k = dense_text.map.partition_point(|m| m.0 < *s);
                    dense_text.chars[..k].ends_with(b)
                })
                .collect(),
            _ => hits.clone(),
        };
        match chosen.len() {
            1 => return Ok(chosen[0]),
            0 => {
                return Err(CoreError::Invalid(format!(
                    "the passage occurs {} time(s), but never right after the given `before` text. Occurrences: {}",
                    hits.len(),
                    contexts(text, &hits)
                )))
            }
            n => {
                return Err(CoreError::Invalid(format!(
                    "the passage occurs {n} times in the paper; quote a longer passage or give `before` (a few words just before it). Occurrences: {}",
                    contexts(text, &chosen)
                )))
            }
        }
    }
    // not found: say how far it does match, to help fix the quote
    let n = dense_q.len();
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if find_all(&dense_text.chars, &dense_q[..mid]).is_empty() {
            hi = mid - 1;
        } else {
            lo = mid;
        }
    }
    let mut msg = "that passage isn't in the paper's reading view; quote its text exactly (e.g. as read_paper shows it)".to_string();
    if lo >= 12 {
        if let Some(&i) = find_all(&dense_text.chars, &dense_q[..lo]).first() {
            let s = dense_text.map[i].0;
            let e = dense_text.map[i + lo - 1].1;
            let after: String = text[e..].chars().take(60).collect();
            msg += &format!(
                ". It matches up to “{}”; there the paper continues “{}”",
                squash(&text[s..e]).chars().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect::<String>(),
                squash(&after)
            );
        }
    }
    Err(CoreError::NotFound(msg))
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn contexts(text: &str, hits: &[(usize, usize)]) -> String {
    hits.iter()
        .take(5)
        .enumerate()
        .map(|(n, (s, _))| {
            let pre: String = text[..*s].chars().rev().take(50).collect::<Vec<_>>().into_iter().rev().collect();
            let post: String = text[*s..].chars().take(70).collect();
            format!("{}. “…{}⟦{}…”", n + 1, squash(&pre), squash(&post))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// An unsaved highlight for the passage at `range` of `text` (the reader's anchoring).
pub fn anchor(paper: &str, text: &str, range: (usize, usize), location: &str) -> Annotation {
    let (s, e) = range;
    let prefix: String = text[..s].chars().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect();
    let suffix: String = text[e..].chars().take(40).collect();
    Annotation {
        id: String::new(),
        paper_id: paper.to_string(),
        kind: AnnotationKind::Highlight,
        color: "yellow".into(),
        location: location.into(),
        position: if location == "epub" { text[..s].encode_utf16().count().to_string() } else { "0".into() },
        quote: text[s..e].to_string(),
        prefix,
        suffix,
        note: String::new(),
        created: 0,
        modified: 0,
    }
}

/// Which highlights `Library::find_annotations` returns. Every given field must match.
#[derive(uniffi::Record, Clone, Debug, Default)]
pub struct AnnotationFilter {
    /// Only this paper's.
    #[uniffi(default = None)]
    pub paper: Option<String>,
    /// "yellow", "green", "blue" or "pink".
    #[uniffi(default = None)]
    pub color: Option<String>,
    /// Only in papers with this tag.
    #[uniffi(default = None)]
    pub tag: Option<String>,
    /// Only in papers in this collection (id).
    #[uniffi(default = None)]
    pub collection: Option<String>,
    /// Words that must all appear in the quote or the note (case-insensitive).
    #[uniffi(default = None)]
    pub query: Option<String>,
}

/// Lower-case, diacritic- and punctuation-light text for word matching.
pub fn matches_query(a: &Annotation, query: &str) -> bool {
    let hay: String = format!("{} {}", a.quote, a.note).chars().flat_map(|c| fold_char(c)).collect();
    let hay = squash(&hay);
    query
        .split_whitespace()
        .map(|w| w.chars().flat_map(fold_char).collect::<String>())
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .all(|w| hay.contains(&w))
}

fn fold_char(c: char) -> Vec<char> {
    match c {
        '‘' | '’' => vec!['\''],
        '“' | '”' => vec!['"'],
        '\u{2010}'..='\u{2015}' | '−' => vec!['-'],
        _ => c.to_lowercase().collect(),
    }
}

pub const COLORS: [&str; 4] = ["yellow", "green", "blue", "pink"];
