//! Resolve Citations: write first, cite later. A writer types placeholders while writing,
//! "(Langseth et al. 2025)", "(Langseth et al., 2025; Smith 2019)", "[Langseth 2025 spatial]"
//! (words after the year are hints), "{Langseth 2025}", in LaTeX `\cite{?langseth2025}` or
//! `\cite{langseth2025?}`; Lunta finds them (`find_placeholders`, `find_tex_placeholders`) and
//! proposes the papers each may mean (`Library::suggest_citation`): the library first (family
//! name, year, hint words), then Crossref (OpenAlex when Crossref has nothing). The writer
//! confirms each one in Word, Google Docs or the LaTeX sheet; nothing here changes a document or
//! the library by itself (`add_found_paper` adds a paper the writer confirmed).
//!
//! What is not a placeholder: ordinary parentheses ("(see Fig. 2)", "(n = 2025)", "(p < 0.05)",
//! "(2019–2020)", "(Table 1)"): every part between the brackets must be family names and a
//! year. Word and Google Docs only pass their plain text (not Lunta's citations) to be scanned.

use crate::model::*;
use crate::{export, metadata, Library};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

#[cfg(test)]
mod tests;

macro_rules! re {
    ($p:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new(&$p).unwrap())
    }};
}

/// How a placeholder was written.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceholderKind {
    /// "(Langseth et al., 2025; Smith 2019)"
    Parentheses,
    /// "[Langseth 2025 spatial]"
    Brackets,
    /// "{Langseth 2025}"
    Braces,
    /// `\cite{?langseth2025}`
    Latex,
}

/// One work a placeholder cites ("Langseth et al., 2025" of "(Langseth et al., 2025; Smith 2019)").
#[derive(uniffi::Record, Clone, Debug, PartialEq, Default)]
pub struct PlaceholderCite {
    /// As written ("Langseth et al., 2025"; in LaTeX the key with its "?").
    pub text: String,
    /// Family names as written, the first author first ("Smith", "Jones" of "Smith and Jones 2019").
    pub surnames: Vec<String>,
    pub et_al: bool,
    /// "2025" ("" when not given, which only a LaTeX key may do).
    pub year: String,
    /// The words after the year, lower case ("spatial"): looked for in titles.
    pub hints: Vec<String>,
    /// LaTeX: the key without its "?" ("langseth2025"); "" otherwise.
    pub key: String,
}

/// A placeholder in a text.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Placeholder {
    /// Where it is in the text, in UTF-16 code units (as JavaScript, Word, Google Docs and
    /// NSString count): `end` is past its last character.
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub kind: PlaceholderKind,
    pub cites: Vec<PlaceholderCite>,
    /// LaTeX: the command up to its keys (`\citep[p.~4]`) and every key as written, the
    /// placeholders' with their "?" (`["?langseth2025", "smith2019"]`); empty otherwise.
    pub command: String,
    pub keys: Vec<String>,
}

/// A paper a placeholder may mean.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct CitationCandidate {
    /// The library's paper id; "" for a paper found outside the library.
    pub paper_id: String,
    pub meta: PaperMeta,
    /// "library", "crossref" or "openalex".
    pub source: String,
    /// 0 to 1: how well the names, year and hint words fit.
    pub confidence: f64,
    /// The only paper in the library with this first author, year and hint words (or the LaTeX
    /// key itself): "Confirm all exact matches" may confirm it with the others.
    pub exact: bool,
    /// The paper's citation key in the library ("" outside it).
    pub cite_key: String,
}

/// The candidates for one cite, best first: the library's, then the ones found outside it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Default)]
pub struct CitationSuggestions {
    pub candidates: Vec<CitationCandidate>,
    /// The first candidate is an exact match (see `CitationCandidate::exact`).
    pub exact: bool,
    /// Something the writer should know ("Crossref couldn't be reached…"), or "".
    pub note: String,
}

/// What the writer chose for a LaTeX file's placeholder: `start`/`end` as found, and one key
/// per cite ("" leaves that cite as written).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PlaceholderChoice {
    pub start: u32,
    pub end: u32,
    pub keys: Vec<String>,
}

const MAX_CANDIDATES: usize = 5;
/// Below this a paper isn't proposed (the first author with a year two off still is).
const MIN_FIT: f64 = 0.15;

// ---------------------------------------------------------------- finding placeholders

/// The placeholders in a document's text (Word, Google Docs): "(Name et al., 2025; Name 2019)",
/// "[Name 2025 hint words]", "{Name 2025}".
#[uniffi::export]
pub fn find_placeholders(text: String) -> Vec<Placeholder> {
    let mut out = bracketed(&text, &text, "([{");
    out.sort_by_key(|p| p.start);
    out
}

/// The placeholders in a LaTeX file: `\cite{?key}` / `\cite{key?}` (any \cite… command, the
/// other keys kept), "(Name et al. 2025)" and "[Name 2025 hint]" ("{…}" is LaTeX's own); not in
/// comments.
#[uniffi::export]
pub fn find_tex_placeholders(text: String) -> Vec<Placeholder> {
    // comments blanked, same byte offsets
    let mut masked = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let mut prev = '\0';
        let mut comment = false;
        for c in line.chars() {
            if !comment && c == '%' && prev != '\\' {
                comment = true;
            }
            if comment && c != '\n' {
                masked.push_str(&" ".repeat(c.len_utf8()));
            } else {
                masked.push(c);
            }
            prev = c;
        }
    }
    let mut out = vec![];
    let mut taken: Vec<(usize, usize)> = vec![];
    let cmd = re!(r"\\[A-Za-z]*(?:cite|Cite)[A-Za-z]*\*?((?:\s*\[[^\]\n]*\]){0,2})\s*\{([^{}]*)\}");
    for m in cmd.captures_iter(&masked) {
        let whole = m.get(0).unwrap();
        taken.push((whole.start(), whole.end()));
        let keys: Vec<String> = m[2].split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
        let cites: Vec<PlaceholderCite> = keys.iter().filter(|k| marked(k)).map(|k| key_cite(k)).collect();
        if cites.is_empty() {
            continue;
        }
        let brace = whole.as_str().rfind('{').unwrap();
        out.push(Placeholder {
            start: utf16(&text, whole.start()),
            end: utf16(&text, whole.end()),
            text: text[whole.start()..whole.end()].to_string(),
            kind: PlaceholderKind::Latex,
            cites,
            command: whole.as_str()[..brace].trim_end().to_string(),
            keys,
        });
    }
    for p in bracketed(&text, &masked, "([") {
        let (s, e) = (byte_of(&text, p.start), byte_of(&text, p.end));
        if !taken.iter().any(|&(a, b)| s < b && a < e) {
            out.push(p);
        }
    }
    out.sort_by_key(|p| p.start);
    out
}

/// `\cite{…}` for the placeholders the writer resolved: each choice's keys go where its cites
/// were (a LaTeX placeholder keeps its command and other keys; "(Name 2025)" becomes
/// `\cite{key}`, any part left as written after it). Choices that no longer match a placeholder
/// at the same place are ignored.
#[uniffi::export]
pub fn rewrite_tex_placeholders(text: String, choices: Vec<PlaceholderChoice>) -> String {
    let found = find_tex_placeholders(text.clone());
    let mut edits: Vec<(usize, usize, String)> = vec![];
    for p in &found {
        let Some(c) = choices.iter().find(|c| c.start == p.start && c.end == p.end && c.keys.len() == p.cites.len()) else { continue };
        let keys: Vec<String> = c.keys.iter().map(|k| k.trim().to_string()).collect();
        if keys.iter().all(|k| k.is_empty()) {
            continue;
        }
        let new = if p.kind == PlaceholderKind::Latex {
            let mut i = 0;
            let mut all: Vec<String> = vec![];
            for k in &p.keys {
                let k = if marked(k) {
                    let chosen = keys[i].clone();
                    i += 1;
                    if chosen.is_empty() { k.clone() } else { chosen }
                } else {
                    k.clone()
                };
                if !all.contains(&k) {
                    all.push(k);
                }
            }
            format!("{}{{{}}}", p.command, all.join(","))
        } else {
            let mut chosen: Vec<String> = vec![];
            for k in keys.iter().filter(|k| !k.is_empty()) {
                if !chosen.contains(k) {
                    chosen.push(k.clone());
                }
            }
            let left: Vec<&str> = p.cites.iter().zip(&keys).filter(|(_, k)| k.is_empty()).map(|(c, _)| c.text.as_str()).collect();
            let mut s = export::latex_cite(&chosen);
            if !left.is_empty() {
                let (o, cl) = if p.kind == PlaceholderKind::Brackets { ('[', ']') } else { ('(', ')') };
                s.push_str(&format!(" {o}{}{cl}", left.join("; ")));
            }
            s
        };
        edits.push((byte_of(&text, p.start), byte_of(&text, p.end), new));
    }
    let mut out = text;
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    for (s, e, new) in edits {
        out.replace_range(s..e, &new);
    }
    out
}

/// "(…)", "[…]", "{…}" (those of `openers`) whose every part is a cite. `masked`: the text to
/// scan (LaTeX: comments blanked; the same byte offsets as `text`).
fn bracketed(text: &str, masked: &str, openers: &str) -> Vec<Placeholder> {
    let mut out = vec![];
    let chars: Vec<(usize, char)> = masked.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (b, c) = chars[i];
        let Some(close) = (match c {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            _ => None,
        })
        .filter(|_| openers.contains(c)) else {
            i += 1;
            continue;
        };
        // the closer, on the same line, not past another opener of the kind, within 300 characters
        let mut j = i + 1;
        let mut end = None;
        while j < chars.len() && j - i < 300 {
            let d = chars[j].1;
            if d == close {
                end = Some(j);
                break;
            }
            if d == c || d == '\n' || d == '\r' {
                break;
            }
            j += 1;
        }
        let Some(j) = end else {
            i += 1;
            continue;
        };
        // "[Smith 2019](https://…)" is a link in Markdown
        let link = c == '[' && chars.get(j + 1).is_some_and(|x| x.1 == '(');
        let e = chars[j].0 + close.len_utf8();
        let inner = &text[b + 1..chars[j].0];
        match (!link).then(|| parts(inner)).flatten() {
            Some(cites) => {
                out.push(Placeholder {
                    start: utf16(text, b),
                    end: utf16(text, e),
                    text: text[b..e].to_string(),
                    kind: match c {
                        '(' => PlaceholderKind::Parentheses,
                        '[' => PlaceholderKind::Brackets,
                        _ => PlaceholderKind::Braces,
                    },
                    cites,
                    command: String::new(),
                    keys: vec![],
                });
                i = j + 1;
            }
            None => i += 1,
        }
    }
    out
}

/// The cites of a placeholder's inside ("Langseth et al., 2025; Smith 2019"), or None when any
/// part isn't one.
fn parts(inner: &str) -> Option<Vec<PlaceholderCite>> {
    let mut out = vec![];
    for part in inner.split(';') {
        out.extend(parse_cite(part)?);
    }
    (!out.is_empty()).then_some(out)
}

/// Words that start like a family name but aren't one: "(Table 2019)", "(January 2020)".
const NOT_NAMES: &[&str] = &[
    "fig", "figs", "figure", "figures", "table", "tables", "tab", "eq", "eqs", "equation", "equations", "section", "sections",
    "sec", "chapter", "chapters", "ch", "appendix", "supplementary", "supplement", "suppl", "box", "panel", "panels", "ref", "refs",
    "reference", "references", "note", "notes", "step", "steps", "phase", "stage", "day", "days", "week", "weeks", "month", "months",
    "year", "years", "version", "release", "model", "group", "groups", "cohort", "study", "studies", "trial", "experiment", "exp",
    "lemma", "theorem", "definition", "corollary", "proposition", "algorithm", "line", "lines", "page", "pages", "vol", "volume",
    "no", "item", "items", "part", "parts", "question", "data", "dataset", "extended", "movie", "video", "code", "listing",
    "footnote", "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
    "december", "jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec", "spring", "summer", "autumn",
    "fall", "winter", "in", "on", "at", "by", "since", "from", "to", "until", "before", "after", "circa", "ca", "about", "around",
    "approx", "approximately", "only", "the", "a", "an", "this", "that", "these", "those", "total", "mean", "median", "range", "age",
    "aged", "born", "est", "founded", "ad", "bc", "bce", "ce", "covid", "sars", "mers", "windows", "python", "iso", "accessed",
    "retrieved", "updated", "published", "last", "first", "next", "both", "all", "each", "every", "some", "many", "most", "see",
    "e", "eg", "ie", "cf", "and", "or", "but", "also", "for", "with", "et", "al", "us", "uk", "eu", "usa", "un", "q", "n", "p",
];

/// Hint words that say nothing about the paper.
const NOT_HINTS: &[&str] = &[
    "the", "and", "for", "with", "from", "into", "that", "this", "these", "see", "also", "page", "pages", "fig", "figure", "table",
    "review", "reviewed", "e.g", "i.e", "for", "about", "paper", "study", "work", "preprint", "et", "al", "chapter", "section",
];

fn cite_re() -> &'static Regex {
    re!({
        let particle = r"(?:(?:van|von|de|der|den|da|di|du|del|della|dos|das|do|la|le|ten|ter|zu|af|bin|ibn|St\.?)\s+)*";
        let word = r"(?:[dDlL]['’])?\p{Lu}[\p{L}\p{M}'’]*(?:-\p{L}[\p{L}\p{M}'’]*)*";
        let name = format!(r"{particle}{word}(?:\s+{word})?");
        let authors = format!(r"{name}(?:\s*,\s*{name})*(?:\s*,?\s*(?:and|&)\s+{name})?");
        format!(
            r"^\s*(?:(?i:see\s+also|see|e\.g\.|cf\.|i\.e\.|also|but\s+see|reviewed\s+in)[\s,]+)?({authors})(\s*,?\s+et\s*al\.?)?\s*,?\s*(1[89]\d\d|20\d\d)([a-z]?)((?:\s*,\s*(?:1[89]\d\d|20\d\d)[a-z]?)*)(?:(?:\s*[,:–—-]\s*|\s+)(.*?))?\s*$"
        )
    })
}

/// One part of a placeholder ("Langseth et al., 2025", "Smith 2019, 2020" = two cites), or None.
fn parse_cite(part: &str) -> Option<Vec<PlaceholderCite>> {
    let c = cite_re().captures(part)?;
    let surnames: Vec<String> = re!(r"\s*(?:,|\band\b|&)\s*")
        .split(&c[1])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if surnames.is_empty() || surnames.iter().any(|s| not_a_name(s)) {
        return None;
    }
    let rest = c.get(6).map(|m| m.as_str()).unwrap_or("");
    // a sentence in brackets, or statistics after a year, isn't a placeholder
    if rest.split_whitespace().count() > 6 || rest.chars().any(|ch| !(ch.is_alphanumeric() || " .,:-–'’~".contains(ch))) {
        return None;
    }
    let year = c[3].to_string();
    if !plausible_year(&year) {
        return None;
    }
    let hints = hint_words(rest);
    let et_al = c.get(2).is_some();
    let mut years = vec![year];
    for y in re!(r"(1[89]\d\d|20\d\d)").find_iter(c.get(5).map(|m| m.as_str()).unwrap_or("")) {
        years.push(y.as_str().to_string());
    }
    let text = part.trim().to_string();
    Some(
        years
            .into_iter()
            .map(|year| PlaceholderCite { text: text.clone(), surnames: surnames.clone(), et_al, year, hints: hints.clone(), key: String::new() })
            .collect(),
    )
}

fn not_a_name(s: &str) -> bool {
    let last = s.split_whitespace().last().unwrap_or("");
    let f = fold(last);
    f.chars().filter(|c| c.is_alphabetic()).count() < 2 || NOT_NAMES.contains(&f.as_str()) || NOT_NAMES.contains(&fold(s).as_str())
}

fn plausible_year(y: &str) -> bool {
    y.parse::<i32>().is_ok_and(|n| (1800..=2100).contains(&n))
}

fn hint_words(rest: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for w in rest.split(|c: char| !c.is_alphanumeric()) {
        let f = fold(w);
        if f.chars().count() >= 3 && f.chars().all(|c| c.is_alphabetic()) && !NOT_HINTS.contains(&f.as_str()) && !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

fn marked(key: &str) -> bool {
    key.len() > 1 && (key.starts_with('?') || key.ends_with('?'))
}

/// A LaTeX placeholder key: "?langseth2025spatial" → Langseth, 2025, hint "spatial".
fn key_cite(raw: &str) -> PlaceholderCite {
    let key = raw.trim_matches('?').trim().to_string();
    let mut surnames = vec![];
    let mut year = String::new();
    let mut hints = vec![];
    let mut et_al = false;
    for run in re!(r"\p{L}+|\d+").find_iter(&key) {
        let t = run.as_str();
        if t.chars().all(|c| c.is_ascii_digit()) {
            if year.is_empty() && t.len() == 4 && plausible_year(t) {
                year = t.to_string();
            }
            continue;
        }
        let f = fold(t);
        if f == "etal" || f == "et" || f == "al" {
            et_al = true;
        } else if surnames.is_empty() {
            // "langseth" or "Langseth": as a name
            let mut cs = t.chars();
            surnames.push(cs.next().map(|c| c.to_uppercase().collect::<String>() + cs.as_str()).unwrap_or_default());
        } else if f.chars().count() >= 3 && !NOT_HINTS.contains(&f.as_str()) && !hints.contains(&f) {
            hints.push(f);
        }
    }
    PlaceholderCite { text: raw.to_string(), surnames, et_al, year, hints, key }
}

fn utf16(text: &str, byte: usize) -> u32 {
    text[..byte].encode_utf16().count() as u32
}

/// The byte offset of a UTF-16 offset (the end of the text when past it).
fn byte_of(text: &str, u: u32) -> usize {
    let mut n = 0u32;
    for (b, c) in text.char_indices() {
        if n >= u {
            return b;
        }
        n += c.len_utf16() as u32;
    }
    text.len()
}

/// Lower case, accents and a few letters as plain ASCII ("Gonçalo" → "goncalo", "Østergaard" → "ostergaard").
pub(crate) fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        let r = match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'æ' => "ae",
            'ç' | 'ć' | 'č' | 'ĉ' => "c",
            'ď' | 'đ' | 'ð' => "d",
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
            'ğ' => "g",
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
            'ł' | 'ľ' | 'ĺ' => "l",
            'ñ' | 'ń' | 'ň' => "n",
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ő' => "o",
            'œ' => "oe",
            'ř' | 'ŕ' => "r",
            'ś' | 'š' | 'ş' | 'ș' => "s",
            'ß' => "ss",
            'ť' | 'ţ' | 'ț' => "t",
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u",
            'ý' | 'ÿ' => "y",
            'ź' | 'ż' | 'ž' => "z",
            'þ' => "th",
            '’' => "'",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(r);
    }
    out
}

// ---------------------------------------------------------------- what a cite may mean

/// The name's parts, folded: "Mattsson Langseth" → ["mattsson", "langseth"], "O'Brien" → ["obrien"].
fn name_parts(s: &str) -> Vec<String> {
    fold(s).split(|c: char| c.is_whitespace() || c == '-').map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>()).filter(|w| !w.is_empty()).collect()
}

/// Whether an author ("Christoffer M. Langseth", "Langseth, C.", "Gonçalo Castelo-Branco") has
/// this family name ("Langseth", "Castelo-Branco", "Castelo", "Mattsson Langseth").
fn same_author(author: &str, surname: &str) -> bool {
    let want = name_parts(surname);
    if want.is_empty() {
        return false;
    }
    let family = name_parts(&export::split_name(author).0);
    if family == want || family.join("") == want.join("") {
        return true;
    }
    // the parts of the whole name but the first (a given name): double family names
    let all = name_parts(author);
    let tail: Vec<&String> = if export::split_name(author).1.is_empty() || author.contains(',') { all.iter().collect() } else { all.iter().skip(1).collect() };
    want.iter().all(|w| tail.iter().any(|t| *t == w)) || family.last() == want.last()
}

/// Whether a hint word is in the text's words ("spatial" in "Spatially resolved…").
fn hint_in(hint: &str, words: &[String]) -> bool {
    words.iter().any(|w| w == hint || (hint.len() >= 4 && w.starts_with(hint)) || (w.len() >= 4 && hint.starts_with(w.as_str())))
}

fn words_of(s: &str) -> Vec<String> {
    fold(s).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// How well a paper fits a cite (0 to 1), and whether it fits it fully: first author, year and
/// every hint word. None when none of its authors has the cite's first family name.
fn fit(cite: &PlaceholderCite, m: &PaperMeta, cite_key: &str) -> Option<(f64, bool)> {
    if !cite.key.is_empty() && !cite_key.is_empty() && cite.key.eq_ignore_ascii_case(cite_key) {
        return Some((1.0, true));
    }
    let first = cite.surnames.first()?;
    let first_author = m.authors.first().is_some_and(|a| same_author(a, first));
    if !first_author && !m.authors.iter().any(|a| same_author(a, first)) {
        return None;
    }
    let mut s: f64 = if first_author { 0.5 } else { 0.15 };
    for (i, other) in cite.surnames.iter().enumerate().skip(1) {
        if m.authors.get(i).is_some_and(|a| same_author(a, other)) {
            s += 0.1;
        } else if m.authors.iter().any(|a| same_author(a, other)) {
            s += 0.05;
        } else {
            s -= 0.15;
        }
    }
    let n = m.authors.len();
    if cite.et_al {
        s += if n >= 3 { 0.05 } else { -0.05 };
    } else if cite.surnames.len() == 1 {
        s += if n == 1 { 0.05 } else if n == 2 { -0.05 } else { 0.0 };
    } else if cite.surnames.len() == 2 && n == 2 {
        s += 0.05;
    }
    let year_ok = match (cite.year.parse::<i32>(), m.year().parse::<i32>()) {
        (Ok(want), Ok(have)) if want == have => {
            s += 0.35;
            true
        }
        // online first, print the next year; a preprint and its paper
        (Ok(want), Ok(have)) if (want - have).abs() == 1 => {
            s += 0.1;
            false
        }
        (Ok(_), Ok(_)) => {
            s -= 0.3;
            false
        }
        _ => false,
    };
    let mut all_hints = true;
    if !cite.hints.is_empty() {
        let title = words_of(&m.title);
        let other = words_of(&format!("{} {} {} {}", m.journal, m.keywords.join(" "), m.abstract_text, m.series));
        let mut got = 0.0;
        for h in &cite.hints {
            if hint_in(h, &title) {
                got += 1.0;
            } else if hint_in(h, &other) {
                got += 0.6;
            } else {
                all_hints = false;
            }
        }
        let f = got / cite.hints.len() as f64;
        s += if f == 0.0 { -0.1 } else { 0.15 * f };
    }
    Some((s.clamp(0.0, 1.0), first_author && year_ok && all_hints))
}

/// The library's candidates for a cite, best first; each exact when it is the only full fit.
fn library_candidates(lib: &Library, cite: &PlaceholderCite) -> Vec<CitationCandidate> {
    let mut scored: Vec<(f64, bool, Paper)> = lib
        .papers(PaperFilter::default(), SortKey::Added)
        .into_iter()
        .filter_map(|p| fit(cite, &p.meta, &p.cite_key).map(|(s, full)| (s, full, p)))
        .filter(|(s, _, _)| *s >= MIN_FIT)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.2.added.cmp(&a.2.added)));
    let key_hit = scored.iter().any(|(s, _, _)| *s >= 1.0);
    let full = scored.iter().filter(|(_, f, _)| *f).count();
    scored
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(s, f, p)| CitationCandidate {
            exact: if key_hit { s >= 1.0 } else { f && full == 1 },
            paper_id: p.id,
            cite_key: p.cite_key,
            meta: p.meta,
            source: "library".into(),
            confidence: s,
        })
        .collect()
}

/// Crossref's search for a cite: its family names, the hint words, the years around its year.
fn crossref_url(cite: &PlaceholderCite) -> String {
    let mut u = format!(
        "https://api.crossref.org/works?rows=10&select=DOI,title,author,issued,container-title,type,volume,issue,page,publisher&query.author={}",
        metadata::enc(&cite.surnames.join(" "))
    );
    if !cite.hints.is_empty() {
        u.push_str(&format!("&query.bibliographic={}", metadata::enc(&cite.hints.join(" "))));
    }
    if let Ok(y) = cite.year.parse::<i32>() {
        u.push_str(&format!("&filter=from-pub-date:{},until-pub-date:{}", y - 1, y + 1));
    }
    u
}

/// OpenAlex's search (when Crossref has nothing): the family name and hint words, the years around.
fn openalex_url(cite: &PlaceholderCite) -> String {
    let q = format!("{} {}", cite.surnames.join(" "), cite.hints.join(" "));
    let mut u = format!(
        "https://api.openalex.org/works?search={}&per_page=10&select=title,authorships,publication_date,primary_location,doi,biblio,type_crossref",
        metadata::enc(q.trim()).replace('/', "%2F")
    );
    if let Ok(y) = cite.year.parse::<i32>() {
        u.push_str(&format!("&filter=publication_year:{}-{}", y - 1, y + 1));
    }
    u
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => a.first().map(s).unwrap_or_default(),
        _ => String::new(),
    }
}

/// The works of a Crossref /works search, as details (what a candidate shows and a citation needs).
fn crossref_works(j: &Value) -> Vec<PaperMeta> {
    let mut out = vec![];
    for it in j["message"]["items"].as_array().into_iter().flatten() {
        let authors = it["author"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|x| {
                        let n = format!("{} {}", s(&x["given"]), s(&x["family"])).trim().to_string();
                        if n.is_empty() { s(&x["name"]) } else { n }
                    })
                    .filter(|n| !n.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let date = it["issued"]["date-parts"][0]
            .as_array()
            .map(|p| p.iter().filter_map(Value::as_i64).enumerate().map(|(i, v)| if i == 0 { format!("{v:04}") } else { format!("{v:02}") }).collect::<Vec<_>>().join("-"))
            .unwrap_or_default();
        let m = PaperMeta {
            title: metadata::strip_tags(&s(&it["title"])),
            authors,
            date,
            journal: metadata::strip_tags(&s(&it["container-title"])),
            volume: s(&it["volume"]),
            issue: s(&it["issue"]),
            pages: s(&it["page"]),
            publisher: s(&it["publisher"]),
            doi: s(&it["DOI"]),
            work_type: s(&it["type"]),
            ..Default::default()
        };
        if !m.title.is_empty() {
            out.push(m);
        }
    }
    out
}

/// The works of an OpenAlex /works search, as details.
fn openalex_works(j: &Value) -> Vec<PaperMeta> {
    let mut out = vec![];
    for w in j["results"].as_array().into_iter().flatten() {
        let b = &w["biblio"];
        let pages = match (s(&b["first_page"]), s(&b["last_page"])) {
            (f, l) if !f.is_empty() && !l.is_empty() && f != l => format!("{f}-{l}"),
            (f, _) => f,
        };
        let m = PaperMeta {
            title: metadata::strip_tags(&s(&w["title"])),
            authors: w["authorships"].as_array().map(|a| a.iter().map(|x| s(&x["author"]["display_name"])).filter(|n| !n.is_empty()).collect()).unwrap_or_default(),
            date: s(&w["publication_date"]),
            journal: s(&w["primary_location"]["source"]["display_name"]),
            volume: s(&b["volume"]),
            issue: s(&b["issue"]),
            pages,
            doi: s(&w["doi"]).trim_start_matches("https://doi.org/").to_string(),
            work_type: s(&w["type_crossref"]),
            ..Default::default()
        };
        if !m.title.is_empty() {
            out.push(m);
        }
    }
    out
}

/// Candidates found outside the library: the works that fit, a little less sure than the
/// library's (the writer cites what they have read), papers the library has made the library's.
fn outside(lib: &Library, cite: &PlaceholderCite, works: Vec<PaperMeta>, source: &str, have: &[CitationCandidate]) -> Vec<CitationCandidate> {
    let mut out: Vec<CitationCandidate> = vec![];
    for m in works {
        let Some((sc, _)) = fit(cite, &m, "") else { continue };
        if sc < MIN_FIT {
            continue;
        }
        let doi = metadata::normalize_doi(&m.doi);
        let seen = |c: &CitationCandidate| (!doi.is_empty() && metadata::normalize_doi(&c.meta.doi) == doi) || (doi.is_empty() && c.meta.title.eq_ignore_ascii_case(&m.title));
        if have.iter().any(seen) || out.iter().any(seen) {
            continue;
        }
        if let Some(p) = lib.find_cited(m.doi.clone(), m.title.clone()) {
            if have.iter().any(|c| c.paper_id == p.id) {
                continue;
            }
            out.push(CitationCandidate { paper_id: p.id, cite_key: p.cite_key, meta: p.meta, source: "library".into(), confidence: sc, exact: false });
            continue;
        }
        out.push(CitationCandidate { paper_id: String::new(), meta: m, source: source.into(), confidence: sc * 0.9, exact: false, cite_key: String::new() });
    }
    out.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    out
}

/// Asks the web for JSON (a stand-in in tests).
pub(crate) type Fetch<'a> = &'a dyn Fn(&str) -> Result<Value>;

pub(crate) fn suggest_with(lib: &Library, cite: &PlaceholderCite, external: bool, fetch: Fetch) -> CitationSuggestions {
    let mut c = library_candidates(lib, cite);
    let exact = c.first().is_some_and(|x| x.exact);
    let mut note = String::new();
    if external && !exact && c.len() < MAX_CANDIDATES && !cite.surnames.is_empty() {
        let mut found = match fetch(&crossref_url(cite)) {
            Ok(j) => outside(lib, cite, crossref_works(&j), "crossref", &c),
            Err(e) => {
                note = format!("Crossref couldn’t be asked ({e}).");
                vec![]
            }
        };
        if found.is_empty() && !cite.hints.is_empty() {
            match fetch(&openalex_url(cite)) {
                Ok(j) => {
                    found = outside(lib, cite, openalex_works(&j), "openalex", &c);
                    note.clear();
                }
                Err(e) if note.is_empty() => note = format!("OpenAlex couldn’t be asked ({e})."),
                Err(_) => note = "Crossref and OpenAlex couldn’t be reached: only your library was searched.".into(),
            }
        }
        // library papers first, then the rest
        let (mine, rest): (Vec<_>, Vec<_>) = found.into_iter().partition(|x| !x.paper_id.is_empty());
        c.extend(mine);
        c.extend(rest);
    }
    c.truncate(MAX_CANDIDATES);
    CitationSuggestions { exact: c.first().is_some_and(|x| x.exact), candidates: c, note }
}

pub(crate) fn search_with(lib: &Library, query: &str, external: bool, fetch: Fetch) -> CitationSuggestions {
    let q = query.trim();
    if q.is_empty() {
        return CitationSuggestions::default();
    }
    if let Some(cite) = parse_cite(q).and_then(|v| v.into_iter().next()) {
        return suggest_with(lib, &cite, external, fetch);
    }
    let mut c: Vec<CitationCandidate> = vec![];
    if let Some(p) = lib.find_by_doi(q.to_string()) {
        c.push(CitationCandidate { paper_id: p.id, cite_key: p.cite_key, meta: p.meta, source: "library".into(), confidence: 1.0, exact: false });
    }
    let hits = lib.search(q.to_string(), MAX_CANDIDATES as u32).unwrap_or_default();
    let n = hits.len().max(1) as f64;
    for (i, h) in hits.into_iter().enumerate() {
        if !c.iter().any(|x| x.paper_id == h.paper.id) {
            c.push(CitationCandidate { paper_id: h.paper.id, cite_key: h.paper.cite_key, meta: h.paper.meta, source: "library".into(), confidence: 0.6 - 0.3 * i as f64 / n, exact: false });
        }
    }
    let mut note = String::new();
    if external && c.len() < MAX_CANDIDATES {
        let url = format!(
            "https://api.crossref.org/works?rows=5&select=DOI,title,author,issued,container-title,type,volume,issue,page,publisher&query.bibliographic={}",
            metadata::enc(q)
        );
        match fetch(&url) {
            Ok(j) => {
                for m in crossref_works(&j) {
                    let doi = metadata::normalize_doi(&m.doi);
                    if c.iter().any(|x| !doi.is_empty() && metadata::normalize_doi(&x.meta.doi) == doi) {
                        continue;
                    }
                    if let Some(p) = lib.find_cited(m.doi.clone(), m.title.clone()) {
                        c.push(CitationCandidate { paper_id: p.id, cite_key: p.cite_key, meta: p.meta, source: "library".into(), confidence: 0.4, exact: false });
                    } else {
                        c.push(CitationCandidate { paper_id: String::new(), meta: m, source: "crossref".into(), confidence: 0.3, exact: false, cite_key: String::new() });
                    }
                }
            }
            Err(e) => note = format!("Crossref couldn’t be asked ({e})."),
        }
    }
    c.truncate(MAX_CANDIDATES);
    CitationSuggestions { candidates: c, exact: false, note }
}

#[uniffi::export]
impl Library {
    /// Up to 5 papers a placeholder's cite may mean, best first: the library's (family names,
    /// year, hint words, a LaTeX key), then, with `external`, Crossref's (OpenAlex's when Crossref
    /// has none). Blocking with `external`. Nothing is added to the library.
    pub fn suggest_citation(&self, cite: PlaceholderCite, external: bool) -> CitationSuggestions {
        suggest_with(self, &cite, external, &|u| metadata::get_json(u))
    }

    /// The panel's Search for a placeholder: "Name 2025 words" as a cite, otherwise the library's
    /// search and Crossref's (blocking with `external`).
    pub fn search_citation(&self, query: String, external: bool) -> CitationSuggestions {
        search_with(self, &query, external, &|u| metadata::get_json(u))
    }

    /// Adds a paper the writer confirmed (a candidate from outside the library): its full record
    /// when it has a DOI (Crossref, Europe PMC), else the details as found. A paper the library
    /// already has (same DOI or title) is returned as it is. Blocking.
    pub fn add_found_paper(&self, meta: PaperMeta) -> Result<Paper> {
        if let Some(p) = self.find_cited(meta.doi.clone(), meta.title.clone()) {
            return Ok(p);
        }
        let mut m = if meta.doi.trim().is_empty() { meta.clone() } else { metadata::resolve(&meta.doi).unwrap_or_else(|_| meta.clone()) };
        if m.title.is_empty() {
            m.title = meta.title.clone();
        }
        if m.authors.is_empty() {
            m.authors = meta.authors.clone();
        }
        if m.date.is_empty() {
            m.date = meta.date.clone();
        }
        if m.title.trim().is_empty() {
            return Err(CoreError::Invalid("A paper needs a title or a DOI to be added.".into()));
        }
        self.add_paper(m, None, None, None)
    }
}
