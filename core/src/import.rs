//! Importing from other reference managers: BibTeX and RIS exports (ReadCube Papers, Zotero,
//! Mendeley, EndNote, JabRef…), including linked PDF paths where the export carries them.

use crate::model::*;

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct ImportEntry {
    pub meta: PaperMeta,
    /// PDF paths mentioned in the export (may not exist on this machine).
    pub files: Vec<String>,
    pub tags: Vec<String>,
    pub note: String,
}

#[derive(uniffi::Record, Clone, Debug, Default)]
pub struct ImportReport {
    pub added: u32,
    pub merged: u32,
    pub skipped: u32,
    pub pdfs_copied: u32,
    /// Ids of papers that were added or updated (to convert into reading views afterwards).
    pub paper_ids: Vec<String>,
}

// ---------------------------------------------------------------- BibTeX

pub fn parse_bibtex(text: &str) -> Vec<ImportEntry> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut strings: Vec<(String, String)> = vec![];
    let mut out = vec![];
    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        i += 1;
        let start = i;
        while i < chars.len() && chars[i].is_alphanumeric() {
            i += 1;
        }
        let kind: String = chars[start..i].iter().collect::<String>().to_lowercase();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || (chars[i] != '{' && chars[i] != '(') {
            continue;
        }
        let open = chars[i];
        let close = if open == '{' { '}' } else { ')' };
        let body_start = i + 1;
        // find the matching close bracket
        let mut depth = 0i32;
        let mut j = i;
        let mut in_quote = false;
        while j < chars.len() {
            let c = chars[j];
            if c == '"' && depth == 1 && (j == 0 || chars[j - 1] != '\\') {
                in_quote = !in_quote;
            } else if !in_quote && c == open {
                depth += 1;
            } else if !in_quote && c == close {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            } else if !in_quote && close == ')' && c == '{' {
                depth += 1; // braces nest inside a (…)-delimited entry
            } else if !in_quote && close == ')' && c == '}' {
                depth -= 1;
            }
            j += 1;
        }
        let body: String = chars[body_start..j.min(chars.len())].iter().collect();
        i = j + 1;
        match kind.as_str() {
            "comment" | "preamble" => continue,
            "string" => {
                if let Some((k, v)) = body.split_once('=') {
                    strings.push((k.trim().to_lowercase(), unwrap_value(v.trim(), &strings)));
                }
                continue;
            }
            _ => {}
        }
        let fields = parse_fields(&body, &strings);
        out.push(entry_from_bib(&kind, &fields));
    }
    out
}

/// Splits `key, name = value, name = {value}, …` into (name, value) pairs.
fn parse_fields(body: &str, strings: &[(String, String)]) -> Vec<(String, String)> {
    let chars: Vec<char> = body.chars().collect();
    // skip the cite key
    let mut i = chars.iter().position(|c| *c == ',').map(|p| p + 1).unwrap_or(chars.len());
    let mut out = vec![];
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        let ns = i;
        while i < chars.len() && chars[i] != '=' {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let name: String = chars[ns..i].iter().collect::<String>().trim().to_lowercase();
        i += 1;
        // value runs to the next top-level comma
        let vs = i;
        let mut depth = 0;
        let mut in_quote = false;
        while i < chars.len() {
            let c = chars[i];
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
            } else if c == '"' && depth == 0 {
                in_quote = !in_quote;
            } else if c == ',' && depth == 0 && !in_quote {
                break;
            }
            i += 1;
        }
        let raw: String = chars[vs..i].iter().collect();
        if !name.is_empty() {
            out.push((name, unwrap_value(raw.trim(), strings)));
        }
    }
    out
}

/// Resolves `{…}`, `"…"`, numbers, @string macros and `#` concatenation, then cleans LaTeX.
fn unwrap_value(v: &str, strings: &[(String, String)]) -> String {
    let mut parts = vec![];
    for piece in split_concat(v) {
        let p = piece.trim();
        if (p.starts_with('{') && p.ends_with('}')) || (p.starts_with('"') && p.ends_with('"') && p.len() >= 2) {
            parts.push(p[1..p.len() - 1].to_string());
        } else if let Some((_, s)) = strings.iter().find(|(k, _)| *k == p.to_lowercase()) {
            parts.push(s.clone());
        } else {
            parts.push(p.to_string());
        }
    }
    delatex(&parts.join(""))
}

fn split_concat(v: &str) -> Vec<String> {
    let mut out = vec![];
    let mut depth = 0;
    let mut in_quote = false;
    let mut cur = String::new();
    for c in v.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            '"' if depth == 0 => in_quote = !in_quote,
            '#' if depth == 0 && !in_quote => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

/// Converts common LaTeX accents and escapes to Unicode and drops grouping braces.
pub fn delatex(s: &str) -> String {
    let accents: &[(&str, char)] = &[
        ("\"", '\u{0308}'), ("'", '\u{0301}'), ("`", '\u{0300}'), ("^", '\u{0302}'), ("~", '\u{0303}'),
        ("=", '\u{0304}'), (".", '\u{0307}'), ("c", '\u{0327}'), ("v", '\u{030C}'), ("u", '\u{0306}'),
        ("H", '\u{030B}'), ("r", '\u{030A}'), ("k", '\u{0328}'),
    ];
    let specials: &[(&str, &str)] = &[
        ("\\textasciitilde", "~"), ("\\textbackslash", "\\"), ("\\textbar", "|"), ("\\textless", "<"), ("\\textgreater", ">"), ("\\o", "ø"), ("\\O", "Ø"), ("\\aa", "å"), ("\\AA", "Å"), ("\\ae", "æ"), ("\\AE", "Æ"), ("\\ss", "ß"),
        ("\\l", "ł"), ("\\L", "Ł"), ("\\i", "ı"), ("\\&", "&"), ("\\%", "%"), ("\\_", "_"), ("\\#", "#"),
        ("\\$", "$"), ("--", "–"), ("\\textendash", "–"), ("\\textemdash", "—"), ("~", "\u{00A0}"),
    ];
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    'outer: while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            // accent: \"o  \"{o}  {\"o}  \c{c}  \'{\i}
            for (cmd, comb) in accents {
                let cmd_chars: Vec<char> = cmd.chars().collect();
                let n = cmd_chars.len();
                if chars[i + 1..].starts_with(&cmd_chars) {
                    let mut k = i + 1 + n;
                    // letter commands (\c, \v, …) must be followed by "{" or a space,
                    // otherwise this is a longer command such as \cite
                    if cmd.chars().all(|c| c.is_alphabetic()) {
                        if k >= chars.len() || !(chars[k] == '{' || chars[k] == ' ') {
                            continue;
                        }
                        while k < chars.len() && chars[k] == ' ' {
                            k += 1;
                        }
                    }
                    let braced = k < chars.len() && chars[k] == '{';
                    if braced {
                        k += 1;
                    }
                    if k < chars.len() && chars[k] == '\\' && k + 1 < chars.len() && chars[k + 1] == 'i' {
                        k += 1; // dotless i
                    }
                    if k < chars.len() && chars[k].is_alphabetic() {
                        let base = if chars[k] == 'ı' { 'i' } else { chars[k] };
                        let composed: String = [base, *comb].iter().collect();
                        out.push_str(&nfc(&composed));
                        k += 1;
                        if braced && k < chars.len() && chars[k] == '}' {
                            k += 1;
                        }
                        i = k;
                        continue 'outer;
                    }
                }
            }
            for (cmd, rep) in specials {
                if cmd.starts_with('\\') {
                    let cc: Vec<char> = cmd.chars().collect();
                    let end = i + cc.len();
                    if chars[i..].starts_with(&cc) && (end >= chars.len() || !chars[end].is_alphabetic() || !cmd[1..].chars().all(|c| c.is_alphabetic())) {
                        out.push_str(rep);
                        i = end;
                        if i < chars.len() && chars[i] == ' ' && cmd[1..].chars().all(|c| c.is_alphabetic()) {
                            i += 1;
                        }
                        continue 'outer;
                    }
                }
            }
            // drop unknown commands like \textit, \emph (keep their argument)
            let mut k = i + 1;
            while k < chars.len() && chars[k].is_alphabetic() {
                k += 1;
            }
            if k > i + 1 {
                i = k;
                continue;
            }
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == '-' && chars.get(i + 1) == Some(&'-') {
            out.push('–');
            i += if chars.get(i + 2) == Some(&'-') { 3 } else { 2 };
            continue;
        }
        if chars[i] == '{' || chars[i] == '}' {
            i += 1;
            continue;
        }
        if chars[i] == '~' {
            out.push(' ');
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Composes base letter + combining mark for the common Latin cases.
fn nfc(s: &str) -> String {
    let table: &[(&str, &str)] = &[
        ("a\u{0308}", "ä"), ("o\u{0308}", "ö"), ("u\u{0308}", "ü"), ("A\u{0308}", "Ä"), ("O\u{0308}", "Ö"), ("U\u{0308}", "Ü"),
        ("e\u{0308}", "ë"), ("i\u{0308}", "ï"), ("a\u{0301}", "á"), ("e\u{0301}", "é"), ("i\u{0301}", "í"), ("o\u{0301}", "ó"),
        ("u\u{0301}", "ú"), ("E\u{0301}", "É"), ("a\u{0300}", "à"), ("e\u{0300}", "è"), ("o\u{0300}", "ò"), ("a\u{0302}", "â"),
        ("e\u{0302}", "ê"), ("o\u{0302}", "ô"), ("n\u{0303}", "ñ"), ("a\u{0303}", "ã"), ("o\u{0303}", "õ"), ("c\u{0327}", "ç"),
        ("C\u{0327}", "Ç"), ("s\u{030C}", "š"), ("c\u{030C}", "č"), ("z\u{030C}", "ž"), ("r\u{030C}", "ř"), ("S\u{030C}", "Š"),
        ("a\u{030A}", "å"), ("A\u{030A}", "Å"), ("o\u{030B}", "ő"), ("u\u{030B}", "ű"), ("e\u{0328}", "ę"), ("a\u{0328}", "ą"),
        ("g\u{0306}", "ğ"), ("e\u{0304}", "ē"), ("z\u{0307}", "ż"),
    ];
    table.iter().find(|(k, _)| *k == s).map(|(_, v)| v.to_string()).unwrap_or_else(|| s.to_string())
}

fn entry_from_bib(kind: &str, f: &[(String, String)]) -> ImportEntry {
    let get = |k: &str| f.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let authors = split_bib_authors(&get("author"));
    let year = get("year");
    let month = month_num(&get("month"));
    let date = if !get("date").is_empty() { get("date") } else if !month.is_empty() && !year.is_empty() { format!("{year}-{month}") } else { year };
    let mut journal = get("journal");
    if journal.is_empty() {
        journal = get("journaltitle");
    }
    if journal.is_empty() && kind != "article" {
        journal = get("booktitle");
    }
    let eprint = get("eprint");
    let arxiv = if get("archiveprefix").eq_ignore_ascii_case("arxiv") || get("eprinttype").eq_ignore_ascii_case("arxiv") { eprint } else { String::new() };
    let doi = crate::metadata::doi_in(&get("doi")).unwrap_or_else(|| crate::metadata::doi_in(&get("url")).unwrap_or_default());
    let meta = PaperMeta {
        title: get("title"),
        authors,
        date,
        journal,
        volume: get("volume"),
        issue: get("number"),
        pages: get("pages").replace('–', "-"),
        publisher: get("publisher"),
        doi,
        pmid: get("pmid"),
        pmcid: get("pmcid"),
        arxiv,
        url: get("url"),
        abstract_text: get("abstract"),
        keywords: split_list(&get("keywords")),
        license: String::new(),
    };
    let mut files = vec![];
    for (n, v) in f {
        if n == "file" || n == "pdf" || n == "local-url" || n.starts_with("bdsk-file") {
            files.extend(pdf_paths(v));
        }
    }
    let mut tags = split_list(&get("mendeley-tags"));
    tags.extend(split_list(&get("tags")));
    ImportEntry { meta, files, tags, note: get("annote") + &get("note") }
}

fn split_bib_authors(s: &str) -> Vec<String> {
    if s.trim().is_empty() {
        return vec![];
    }
    let re = regex::Regex::new(r"(?i)\s+and\s+").unwrap();
    re.split(s)
        .map(|a| {
            let a = a.trim();
            // "Family, Given" -> "Given Family"
            match a.split_once(',') {
                Some((fam, giv)) if !giv.trim().is_empty() => format!("{} {}", giv.trim(), fam.trim()),
                _ => a.trim_end_matches(',').to_string(),
            }
        })
        .filter(|a| !a.is_empty() && a.to_lowercase() != "others")
        .collect()
}

fn split_list(s: &str) -> Vec<String> {
    s.split([',', ';'])
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn month_num(m: &str) -> String {
    let m = m.trim().to_lowercase();
    let names = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    if let Some(i) = names.iter().position(|n| m.starts_with(n)) {
        return format!("{:02}", i + 1);
    }
    m.parse::<u32>().ok().filter(|n| (1..=12).contains(n)).map(|n| format!("{n:02}")).unwrap_or_default()
}

/// Pulls PDF paths out of Zotero ("desc:/path.pdf:application/pdf"), Mendeley (":path:pdf"),
/// JabRef and plain-path file fields.
pub fn pdf_paths(v: &str) -> Vec<String> {
    let mut out = vec![];
    for item in v.split(';') {
        let item = item.trim().trim_start_matches("file://");
        if item.is_empty() {
            continue;
        }
        // try "a:b:c" segments first, longest segment ending in .pdf wins
        let segs: Vec<&str> = item.split(':').collect();
        let found = segs.iter().find(|s| s.to_lowercase().ends_with(".pdf")).map(|s| s.to_string());
        let path = match found {
            Some(p) => p,
            None if item.to_lowercase().ends_with(".pdf") => item.to_string(),
            None => continue,
        };
        let p = path.replace("\\:", ":").replace("%20", " ");
        let p = if p.starts_with('/') || p.starts_with('~') { p } else { format!("/{p}") };
        out.push(p);
    }
    out
}

// ---------------------------------------------------------------- RIS

pub fn parse_ris(text: &str) -> Vec<ImportEntry> {
    let mut out = vec![];
    let mut cur: Vec<(String, String)> = vec![];
    let mut last_tag = String::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        let tagged = line.len() >= 6 && line.as_bytes()[2] == b' ' && line.as_bytes()[3] == b' ' && line.as_bytes()[4] == b'-'
            && line[..2].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        if tagged {
            let tag = line[..2].to_string();
            let val = line[6.min(line.len())..].trim().to_string();
            if tag == "ER" {
                if !cur.is_empty() {
                    out.push(entry_from_ris(&cur));
                }
                cur.clear();
                continue;
            }
            last_tag = tag.clone();
            cur.push((tag, val));
        } else if !line.trim().is_empty() {
            // continuation line (long abstracts)
            if let Some(l) = cur.last_mut() {
                if l.0 == last_tag {
                    l.1.push(' ');
                    l.1.push_str(line.trim());
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(entry_from_ris(&cur));
    }
    out
}

fn entry_from_ris(f: &[(String, String)]) -> ImportEntry {
    let get = |ks: &[&str]| f.iter().find(|(n, _)| ks.contains(&n.as_str())).map(|(_, v)| v.clone()).unwrap_or_default();
    let all = |ks: &[&str]| f.iter().filter(|(n, _)| ks.contains(&n.as_str())).map(|(_, v)| v.clone()).collect::<Vec<_>>();
    let authors = all(&["AU", "A1"])
        .into_iter()
        .map(|a| match a.split_once(',') {
            Some((fam, giv)) if !giv.trim().is_empty() => format!("{} {}", giv.trim(), fam.trim()),
            _ => a,
        })
        .collect();
    let py = get(&["PY", "Y1", "DA"]);
    let date = py.split('/').filter(|x| !x.is_empty()).take(3).collect::<Vec<_>>().join("-");
    let sp = get(&["SP"]);
    let ep = get(&["EP"]);
    let pages = if ep.is_empty() { sp } else { format!("{sp}-{ep}") };
    let doi = crate::metadata::doi_in(&get(&["DO", "DI"])).unwrap_or_default();
    let mut files = vec![];
    for v in all(&["L1", "L4", "UR"]) {
        if v.to_lowercase().ends_with(".pdf") && !v.starts_with("http") {
            files.extend(pdf_paths(&v));
        }
    }
    ImportEntry {
        meta: PaperMeta {
            title: get(&["TI", "T1", "CT"]),
            authors,
            date,
            journal: get(&["JO", "T2", "JF", "JA"]),
            volume: get(&["VL"]),
            issue: get(&["IS"]),
            pages,
            publisher: get(&["PB"]),
            doi,
            url: all(&["UR"]).into_iter().find(|u| u.starts_with("http")).unwrap_or_default(),
            abstract_text: get(&["AB", "N2"]),
            keywords: all(&["KW"]),
            ..Default::default()
        },
        files,
        tags: vec![],
        note: get(&["N1"]),
    }
}

/// Parses an export file, picking the format from its content.
pub fn parse_any(text: &str) -> Vec<ImportEntry> {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("TY  -") || t.contains("\nTY  -") {
        parse_ris(t)
    } else {
        parse_bibtex(t)
    }
}
