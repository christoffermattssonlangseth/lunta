//! BibTeX, RIS and formatted citations.

use crate::model::*;

/// Splits "Given Middle Family" into (family, given). Handles "Family, Given" and particles.
pub fn split_name(name: &str) -> (String, String) {
    let n = name.trim();
    if let Some((fam, giv)) = n.split_once(',') {
        return (fam.trim().to_string(), giv.trim().to_string());
    }
    let parts: Vec<&str> = n.split_whitespace().collect();
    if parts.len() <= 1 {
        return (n.to_string(), String::new());
    }
    let particles = ["van", "von", "de", "der", "den", "da", "di", "du", "del", "della", "la", "le", "ten", "ter"];
    let mut start = parts.len() - 1;
    while start > 1 && particles.contains(&parts[start - 1].to_lowercase().as_str()) {
        start -= 1;
    }
    (parts[start..].join(" "), parts[..start].join(" "))
}

/// "Christoffer M." -> "C. M."
fn initials(given: &str, with_dots: bool, spaced: bool) -> String {
    let parts: Vec<String> = given
        .split(|c: char| c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(|p| {
            p.split('-')
                .filter_map(|q| q.chars().next())
                .map(|c| if with_dots { format!("{c}.") } else { c.to_string() })
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect();
    parts.join(if spaced { " " } else { "" })
}

/// Latin letters with accents (and a few ligatures) as plain ASCII, the rest of the text as is.
fn ascii_fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let r = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => "A",
            'æ' => "ae", 'Æ' => "Ae", 'œ' => "oe", 'Œ' => "Oe", 'ß' => "ss", 'þ' => "th", 'Þ' => "Th",
            'ç' | 'ć' | 'č' | 'ĉ' => "c", 'Ç' | 'Ć' | 'Č' | 'Ĉ' => "C",
            'ď' | 'đ' | 'ð' => "d", 'Ď' | 'Đ' | 'Ð' => "D",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e", 'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ė' | 'Ę' | 'Ě' => "E",
            'ğ' | 'ģ' => "g", 'Ğ' | 'Ģ' => "G",
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i", 'Ì' | 'Í' | 'Î' | 'Ï' | 'Ī' | 'Į' | 'İ' => "I",
            'ķ' => "k", 'Ķ' => "K",
            'ł' | 'ľ' | 'ļ' | 'ĺ' => "l", 'Ł' | 'Ľ' | 'Ļ' | 'Ĺ' => "L",
            'ñ' | 'ń' | 'ň' | 'ņ' => "n", 'Ñ' | 'Ń' | 'Ň' | 'Ņ' => "N",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o", 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ő' => "O",
            'ŕ' | 'ř' => "r", 'Ŕ' | 'Ř' => "R",
            'ś' | 'š' | 'ş' | 'ș' => "s", 'Ś' | 'Š' | 'Ş' | 'Ș' => "S",
            'ť' | 'ţ' | 'ț' => "t", 'Ť' | 'Ţ' | 'Ț' => "T",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u", 'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' | 'Ů' | 'Ű' | 'Ų' => "U",
            'ý' | 'ÿ' => "y", 'Ý' | 'Ÿ' => "Y",
            'ź' | 'ż' | 'ž' => "z", 'Ź' | 'Ż' | 'Ž' => "Z",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(r);
    }
    out
}

/// Only ASCII letters and digits (accents folded first): "Müller-Lüdenscheidt" → "MullerLudenscheidt".
fn key_part(s: &str) -> String {
    ascii_fold(s).chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

fn first_word(title: &str) -> String {
    let stop = ["a", "an", "the", "of", "on", "in", "for", "and", "to", "with"];
    title
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| key_part(w).to_lowercase())
        .find(|w| !w.is_empty() && !stop.contains(&w.as_str()))
        .unwrap_or_else(|| "paper".into())
}

/// The key a paper's details suggest: first author's family name, year and the first word of
/// the title ("Kukanja2024cellular"), in characters BibTeX takes everywhere. A paper in the
/// library keeps the key it got (`Paper::cite_key`); this is only where that key starts.
pub fn cite_key(m: &PaperMeta) -> String {
    let fam = m.authors.first().map(|a| key_part(&split_name(a).0)).unwrap_or_default();
    let fam = if fam.is_empty() { "anon".to_string() } else { fam };
    let year: String = m.year().chars().filter(|c| c.is_ascii_digit()).collect();
    format!("{}{}{}", fam, year, first_word(&m.title))
}

/// The disambiguating suffix for the n-th paper (from 1) with the same key: a, b, …, z, aa, ab, …
pub fn key_suffix(n: usize) -> String {
    let mut n = n;
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'a' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// `base`, or `base` with the first suffix that makes it unused (`taken` holds lowercased keys:
/// BibTeX and biber treat keys that differ only in case as the same entry).
pub fn unique_key(base: &str, taken: &std::collections::HashSet<String>) -> String {
    if !taken.contains(&base.to_lowercase()) {
        return base.to_string();
    }
    (1..).map(|n| format!("{base}{}", key_suffix(n))).find(|k| !taken.contains(&k.to_lowercase())).unwrap()
}

/// A citation key as typed, trimmed, or why it can't be one: ASCII letters, digits and
/// `- _ : . + /` only, so it works in `\cite{…}` with BibTeX and biber alike.
pub fn check_cite_key(key: &str) -> Result<String> {
    let k = key.trim();
    if k.is_empty() {
        return Err(CoreError::Invalid("The key can't be empty.".into()));
    }
    if k.len() > 100 {
        return Err(CoreError::Invalid("The key is too long.".into()));
    }
    if let Some(c) = k.chars().find(|c| !(c.is_ascii_alphanumeric() || "-_:.+/".contains(*c))) {
        let what = if c.is_whitespace() { "spaces".to_string() } else { format!("“{c}”") };
        return Err(CoreError::Invalid(format!("Keys can't contain {what}: use letters, digits and - _ : . + /")));
    }
    Ok(k.to_string())
}

/// `\cite{a,b,c}` for keys.
pub fn latex_cite(keys: &[String]) -> String {
    format!("\\cite{{{}}}", keys.join(","))
}

fn bib_escape(s: &str) -> String {
    s.replace('\\', "\\textbackslash{}")
        .replace('&', "\\&")
        .replace('%', "\\%")
        .replace('#', "\\#")
        .replace('_', "\\_")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

/// The BibTeX entry type for a paper's kind, as BibTeX and biblatex users expect: conference
/// papers (also LNCS chapters) `@inproceedings`, other chapters `@incollection`, preprints
/// `@misc` with the eprint, reports and standards `@techreport`, theses `@phdthesis`.
pub fn bibtex_type(m: &PaperMeta) -> &'static str {
    use crate::worktype::Kind::*;
    match crate::worktype::kind(m) {
        Article => "article",
        Conference => "inproceedings",
        Chapter if crate::worktype::chapter_in_proceedings(m) => "inproceedings",
        Chapter => "incollection",
        Book => "book",
        Report | Standard => "techreport",
        Thesis => "phdthesis",
        Preprint | Dataset | Other => "misc",
    }
}

/// An author as BibTeX takes it: "Family, Given", an organization whole in braces.
fn bib_name(a: &str) -> String {
    if crate::worktype::is_organization(a) {
        return format!("{{{}}}", bib_escape(a));
    }
    let (f, g) = split_name(a);
    if g.is_empty() { bib_escape(&f) } else { format!("{}, {}", bib_escape(&f), bib_escape(&g)) }
}

pub fn bibtex(papers: &[Paper]) -> String {
    use crate::worktype::Kind;
    // papers from the library carry their fixed key; others (cited in a document only) get one
    // from their details, made unique within this export
    let mut taken: std::collections::HashSet<String> =
        papers.iter().filter(|p| !p.cite_key.is_empty()).map(|p| p.cite_key.to_lowercase()).collect();
    let mut out = String::new();
    for p in papers {
        let key = if p.cite_key.is_empty() {
            let k = unique_key(&cite_key(&p.meta), &taken);
            taken.insert(k.to_lowercase());
            k
        } else {
            p.cite_key.clone()
        };
        let m = &crate::worktype::for_export(&p.meta);
        let kind = crate::worktype::kind(m);
        let entry = bibtex_type(m);
        out += &format!("@{entry}{{{key},\n");
        let mut field = |k: &str, v: &str| {
            if !v.trim().is_empty() {
                out += &format!("  {k} = {{{v}}},\n");
            }
        };
        field("title", &format!("{{{}}}", bib_escape(&m.title)));
        let authors: Vec<String> = m.authors.iter().map(|a| bib_name(a)).collect();
        field("author", &authors.join(" and "));
        let journal = bib_escape(&m.journal);
        match entry {
            "article" => field("journal", &journal),
            "inproceedings" | "incollection" => {
                field("booktitle", &journal);
                field("series", &bib_escape(&m.series));
            }
            "book" => field("series", &bib_escape(if m.series.is_empty() { &m.journal } else { &m.series })),
            "techreport" => field("institution", &bib_escape(if m.publisher.is_empty() { &m.journal } else { &m.publisher })),
            "phdthesis" => field("school", &bib_escape(if m.publisher.is_empty() { &m.journal } else { &m.publisher })),
            _ => match kind {
                // plain BibTeX styles (plain, unsrt) show howpublished, not the eprint
                Kind::Preprint if !m.arxiv.is_empty() => field("howpublished", &format!("arXiv preprint arXiv:{}", m.arxiv)),
                Kind::Preprint if !m.journal.is_empty() => field("howpublished", &format!("{journal} preprint")),
                _ => field("howpublished", &journal),
            },
        }
        field("year", &m.year());
        field("volume", &m.volume);
        field("number", &m.issue);
        field("pages", &m.pages.replace('-', "--").replace('–', "--"));
        if !matches!(entry, "article" | "misc" | "techreport" | "phdthesis") {
            field("publisher", &bib_escape(&m.publisher));
        }
        field("doi", &m.doi);
        field("pmid", &m.pmid);
        if !m.arxiv.is_empty() {
            field("eprint", &m.arxiv);
            field("archivePrefix", "arXiv");
        }
        field("url", &m.url);
        out += "}\n\n";
    }
    out
}

/// The RIS type for a paper's kind.
pub fn ris_type(m: &PaperMeta) -> &'static str {
    use crate::worktype::Kind::*;
    match crate::worktype::kind(m) {
        Article => "JOUR",
        Conference => "CPAPER",
        Chapter => "CHAP",
        Book => "BOOK",
        Report => "RPRT",
        Standard => "STAND",
        Thesis => "THES",
        Dataset => "DATA",
        Preprint | Other => if m.journal.is_empty() { "GEN" } else { "JOUR" },
    }
}

pub fn ris(papers: &[Paper]) -> String {
    let mut out = String::new();
    for p in papers {
        let m = &crate::worktype::for_export(&p.meta);
        let ty = ris_type(m);
        let mut line = |k: &str, v: &str| {
            if !v.trim().is_empty() {
                out += &format!("{k}  - {}\r\n", v.trim());
            }
        };
        line("TY", ty);
        for a in &m.authors {
            let (f, g) = split_name(a);
            line("AU", &if g.is_empty() || crate::worktype::is_organization(a) { a.clone() } else { format!("{f}, {g}") });
        }
        line("TI", &m.title);
        match ty {
            // the book or proceedings title, and its series
            "CPAPER" | "CHAP" => {
                line("T2", &m.journal);
                line("T3", &m.series);
            }
            "BOOK" => line("T3", if m.series.is_empty() { &m.journal } else { &m.series }),
            "RPRT" | "STAND" | "THES" | "DATA" | "GEN" => line("T2", &m.journal),
            _ => line("JO", &m.journal),
        }
        line("PY", &m.year());
        line("DA", &m.date.replace('-', "/"));
        line("VL", &m.volume);
        line("IS", &m.issue);
        let (sp, ep) = m.pages.split_once(['-', '–']).unwrap_or((m.pages.as_str(), ""));
        line("SP", sp);
        line("EP", ep);
        line("PB", &m.publisher);
        line("DO", &m.doi);
        line("UR", &m.url);
        line("AB", &m.abstract_text);
        for k in &m.keywords {
            line("KW", k);
        }
        out += "ER  - \r\n\r\n";
    }
    out
}

/// A conference paper or chapter: cited "In *Book title*", without volume and issue.
fn in_book(m: &PaperMeta) -> bool {
    matches!(crate::worktype::kind(m), crate::worktype::Kind::Conference | crate::worktype::Kind::Chapter)
}

pub fn citation(p: &Paper, style: CitationStyle) -> String {
    // titles and names in capitals in proper case (the library keeps them as they are)
    let fixed = Paper { meta: crate::worktype::for_export(&p.meta), ..p.clone() };
    styled(&fixed, style)
}

fn styled(p: &Paper, style: CitationStyle) -> String {
    let m = &p.meta;
    let book = in_book(m);
    let names: Vec<(String, String)> = m.authors.iter().map(|a| split_name(a)).collect();
    let doi = if m.doi.is_empty() { String::new() } else { format!("https://doi.org/{}", m.doi) };
    let pages = m.pages.replace('-', "–");
    match style {
        CitationStyle::Apa => {
            let fmt = |(f, g): &(String, String)| format!("{f}, {}", initials(g, true, true)).trim_end_matches(", ").to_string();
            let authors = match names.len() {
                0 => String::new(),
                1 => fmt(&names[0]),
                n if n <= 20 => {
                    let head: Vec<String> = names[..n - 1].iter().map(fmt).collect();
                    format!("{}, & {}", head.join(", "), fmt(&names[n - 1]))
                }
                n => {
                    let head: Vec<String> = names[..19].iter().map(fmt).collect();
                    format!("{}, … {}", head.join(", "), fmt(&names[n - 1]))
                }
            };
            let mut s = format!("{} ({}). {}.", authors, if m.year().is_empty() { "n.d.".into() } else { m.year() }, m.title.trim_end_matches('.'));
            if book && !m.journal.is_empty() {
                s += &format!(" In *{}*", m.journal);
                if !pages.is_empty() {
                    s += &format!(" (pp. {pages})");
                }
                s += ".";
                if !m.publisher.is_empty() {
                    s += &format!(" {}.", m.publisher);
                }
            } else if !m.journal.is_empty() {
                s += &format!(" *{}*", m.journal);
                if !m.volume.is_empty() {
                    s += &format!(", *{}*", m.volume);
                    if !m.issue.is_empty() {
                        s += &format!("({})", m.issue);
                    }
                }
                if !pages.is_empty() {
                    s += &format!(", {pages}");
                }
                s += ".";
            }
            if !doi.is_empty() {
                s += &format!(" {doi}");
            }
            s
        }
        CitationStyle::Vancouver => {
            let list: Vec<String> = names.iter().take(6).map(|(f, g)| format!("{f} {}", initials(g, false, false)).trim().to_string()).collect();
            let authors = if names.len() > 6 { format!("{}, et al", list.join(", ")) } else { list.join(", ") };
            let mut s = format!("{}. {}.", authors, m.title.trim_end_matches('.'));
            if book && !m.journal.is_empty() {
                s += &format!(" In: {}.", m.journal);
                if !m.publisher.is_empty() {
                    s += &format!(" {};", m.publisher);
                }
                s += &format!(" {}.", m.year());
                if !pages.is_empty() {
                    s += &format!(" p. {}.", m.pages);
                }
            } else if !m.journal.is_empty() {
                s += &format!(" {}. {}", m.journal, m.year());
                if !m.volume.is_empty() {
                    s += &format!(";{}", m.volume);
                    if !m.issue.is_empty() {
                        s += &format!("({})", m.issue);
                    }
                }
                if !pages.is_empty() {
                    s += &format!(":{}", m.pages);
                }
                s += ".";
            }
            if !m.doi.is_empty() {
                s += &format!(" doi:{}", m.doi);
            }
            s
        }
        CitationStyle::Nature => {
            let fmt = |(f, g): &(String, String)| format!("{f}, {}", initials(g, true, true)).trim_end_matches(", ").to_string();
            let authors = match names.len() {
                0 => String::new(),
                1 => fmt(&names[0]),
                n if n <= 5 => {
                    let head: Vec<String> = names[..n - 1].iter().map(fmt).collect();
                    format!("{} & {}", head.join(", "), fmt(&names[n - 1]))
                }
                _ => format!("{} et al.", fmt(&names[0])),
            };
            let mut s = format!("{} {}.", authors, m.title.trim_end_matches('.'));
            if book && !m.journal.is_empty() {
                s += &format!(" in *{}*", m.journal);
                if !pages.is_empty() {
                    s += &format!(" {pages}");
                }
                let by = [m.publisher.clone(), m.year()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
                s += &format!(" ({by}).");
                return s;
            }
            if !m.journal.is_empty() {
                s += &format!(" *{}*", m.journal);
                if !m.volume.is_empty() {
                    s += &format!(" **{}**", m.volume);
                }
                if !pages.is_empty() {
                    s += &format!(", {pages}");
                }
            }
            s += &format!(" ({}).", m.year());
            s
        }
        _ => more_citation(p, style, book),
    }
}

/// "a, b and c" / "a, b, and c" (serial comma).
fn and_list(items: &[String], and: &str, serial: bool) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} {and} {}", items[0], items[1]),
        n => format!("{}{} {and} {}", items[..n - 1].join(", "), if serial { "," } else { "" }, items[n - 1]),
    }
}

/// The styles added after APA, Vancouver and Nature. Markdown *italic* and **bold**, as above.
fn more_citation(p: &Paper, style: CitationStyle, book: bool) -> String {
    let m = &p.meta;
    let names: Vec<(String, String)> = m.authors.iter().map(|a| split_name(a)).collect();
    let year = if m.year().is_empty() { "n.d.".to_string() } else { m.year() };
    let title = m.title.trim().trim_end_matches('.').to_string();
    let pages = m.pages.replace('-', "–");
    let doi_url = if m.doi.is_empty() { String::new() } else { format!("https://doi.org/{}", m.doi) };
    // "Langseth, C.M." / "C. M. Langseth" / "Langseth CM" / "Langseth, Christoffer M."
    let fam_init = |(f, g): &(String, String), spaced: bool| {
        let i = initials(g, true, spaced);
        if i.is_empty() { f.clone() } else { format!("{f}, {i}") }
    };
    let init_fam = |(f, g): &(String, String)| {
        let i = initials(g, true, true);
        if i.is_empty() { f.clone() } else { format!("{i} {f}") }
    };
    let full_inverted = |(f, g): &(String, String)| if g.is_empty() { f.clone() } else { format!("{f}, {g}") };
    let full = |(f, g): &(String, String)| if g.is_empty() { f.clone() } else { format!("{g} {f}") };
    let end = |s: &mut String| {
        if !s.ends_with('.') { s.push('.') }
    };

    match style {
        // Harvard (Cite Them Right): up to three authors, then "et al."
        CitationStyle::Harvard => {
            let authors = if names.len() > 3 {
                format!("{} et al.", fam_init(&names[0], false))
            } else {
                and_list(&names.iter().map(|n| fam_init(n, false)).collect::<Vec<_>>(), "and", false)
            };
            let mut s = format!("{authors} ({year}) ‘{title}’");
            if book && !m.journal.is_empty() {
                s += &format!(", in *{}*", m.journal);
                if !m.publisher.is_empty() { s += &format!(". {}", m.publisher); }
                if !pages.is_empty() { s += &format!(", pp. {pages}"); }
            } else if !m.journal.is_empty() {
                s += &format!(", *{}*", m.journal);
                if !m.volume.is_empty() {
                    s += &format!(", {}", m.volume);
                    if !m.issue.is_empty() { s += &format!("({})", m.issue); }
                }
                if !pages.is_empty() { s += &format!(", pp. {pages}"); }
            }
            s += ".";
            if !doi_url.is_empty() { s += &format!(" Available at: {doi_url}."); }
            s
        }
        // Chicago author-date (17th): up to ten authors, else the first seven and "et al."
        CitationStyle::Chicago => {
            let shown: Vec<&(String, String)> = if names.len() > 10 { names.iter().take(7).collect() } else { names.iter().collect() };
            let list: Vec<String> = shown.iter().enumerate().map(|(i, n)| if i == 0 { full_inverted(n) } else { full(n) }).collect();
            let mut authors = if names.len() > 10 { format!("{}, et al", list.join(", ")) } else { and_list(&list, "and", true) };
            end(&mut authors);
            let mut s = format!("{authors} {year}. “{title}.”");
            if book && !m.journal.is_empty() {
                s += &format!(" In *{}*", m.journal);
                if !pages.is_empty() { s += &format!(", {pages}"); }
                s += ".";
                if !m.publisher.is_empty() { s += &format!(" {}.", m.publisher); }
            } else if !m.journal.is_empty() {
                s += &format!(" *{}*", m.journal);
                if !m.volume.is_empty() {
                    s += &format!(" {}", m.volume);
                    if !m.issue.is_empty() { s += &format!(" ({})", m.issue); }
                }
                if !pages.is_empty() { s += &format!(": {pages}"); }
                s += ".";
            }
            if !doi_url.is_empty() { s += &format!(" {doi_url}."); }
            s
        }
        // MLA 9: one author, two ("and"), or the first and "et al."
        CitationStyle::Mla => {
            let mut authors = match names.len() {
                0 => String::new(),
                1 => full_inverted(&names[0]),
                2 => format!("{}, and {}", full_inverted(&names[0]), full(&names[1])),
                _ => format!("{}, et al", full_inverted(&names[0])),
            };
            if !authors.is_empty() { end(&mut authors); authors.push(' '); }
            let mut s = format!("{authors}“{title}.”");
            if book && !m.journal.is_empty() {
                s += &format!(" *{}*", m.journal);
                if !m.publisher.is_empty() { s += &format!(", {}", m.publisher); }
                s += &format!(", {year}");
                if !pages.is_empty() { s += &format!(", pp. {pages}"); }
            } else if !m.journal.is_empty() {
                s += &format!(" *{}*", m.journal);
                if !m.volume.is_empty() { s += &format!(", vol. {}", m.volume); }
                if !m.issue.is_empty() { s += &format!(", no. {}", m.issue); }
                s += &format!(", {year}");
                if !pages.is_empty() { s += &format!(", pp. {pages}"); }
            } else {
                s += &format!(" {year}");
            }
            if !doi_url.is_empty() { s += &format!(", {doi_url}"); }
            s += ".";
            s
        }
        // Cell Press: up to ten authors, else the first ten and "et al."; volume in italics
        CitationStyle::Cell => {
            let list: Vec<String> = names.iter().take(10).map(|n| fam_init(n, false)).collect();
            let authors = if names.len() > 10 { format!("{}, et al.", list.join(", ")) } else { and_list(&list, "and", true) };
            let mut s = format!("{authors} ({year}). {title}.");
            if book && !m.journal.is_empty() {
                s += &format!(" In {}", m.journal);
                if !pages.is_empty() { s += &format!(", pp. {pages}"); }
                s += ".";
                if !m.publisher.is_empty() { s += &format!(" {}.", m.publisher); }
            } else if !m.journal.is_empty() {
                s += &format!(" {}", m.journal);
                if !m.volume.is_empty() { s += &format!(" *{}*", m.volume); }
                if !pages.is_empty() { s += &format!(", {pages}"); }
                s += ".";
            }
            if !doi_url.is_empty() { s += &format!(" {doi_url}."); }
            s
        }
        // IEEE: up to six authors, else the first and "*et al.*"
        CitationStyle::Ieee => {
            let authors = if names.len() > 6 {
                format!("{} *et al.*", init_fam(&names[0]))
            } else {
                and_list(&names.iter().map(init_fam).collect::<Vec<_>>(), "and", true)
            };
            let mut s = format!("{authors}, “{title},”");
            if book && !m.journal.is_empty() {
                s += &format!(" in *{}*", m.journal);
                if !m.publisher.is_empty() { s += &format!(", {}", m.publisher); }
                s += &format!(", {year}");
                if !pages.is_empty() { s += &format!(", pp. {pages}"); }
                if !m.doi.is_empty() { s += &format!(", doi: {}", m.doi); }
                s += ".";
                return s;
            }
            if !m.journal.is_empty() {
                s += &format!(" *{}*", m.journal);
                if !m.volume.is_empty() { s += &format!(", vol. {}", m.volume); }
                if !m.issue.is_empty() { s += &format!(", no. {}", m.issue); }
                if !pages.is_empty() { s += &format!(", pp. {pages}"); }
                s += ",";
            }
            s += &format!(" {year}");
            if !m.doi.is_empty() { s += &format!(", doi: {}", m.doi); }
            s += ".";
            s
        }
        // AMA (11th): up to six authors, else the first three and "et al"
        CitationStyle::Ama => {
            let fmt = |(f, g): &(String, String)| format!("{f} {}", initials(g, false, false)).trim().to_string();
            let authors = if names.len() > 6 {
                format!("{}, et al", names.iter().take(3).map(fmt).collect::<Vec<_>>().join(", "))
            } else {
                names.iter().map(fmt).collect::<Vec<_>>().join(", ")
            };
            let mut s = if authors.is_empty() { format!("{title}.") } else { format!("{authors}. {title}.") };
            if book && !m.journal.is_empty() {
                s += &format!(" In: *{}*.", m.journal);
                if !m.publisher.is_empty() { s += &format!(" {};", m.publisher); }
                s += &format!(" {year}");
                if !m.pages.is_empty() { s += &format!(":{}", m.pages); }
                s += ".";
            } else if !m.journal.is_empty() {
                s += &format!(" *{}*. {year}", m.journal);
                if !m.volume.is_empty() {
                    s += &format!(";{}", m.volume);
                    if !m.issue.is_empty() { s += &format!("({})", m.issue); }
                }
                if !m.pages.is_empty() { s += &format!(":{}", m.pages); }
                s += ".";
            }
            if !m.doi.is_empty() { s += &format!(" doi:{}", m.doi); }
            s
        }
        // Science and PNAS: up to five authors, else the first and "et al."; volume in bold
        CitationStyle::Science | CitationStyle::Pnas => {
            let authors = if names.len() > 5 {
                format!("{} et al.", init_fam(&names[0]))
            } else {
                names.iter().map(init_fam).collect::<Vec<_>>().join(", ")
            };
            let mut s = format!("{authors}, {title}.");
            if book && !m.journal.is_empty() {
                s += &format!(" in *{}*", m.journal);
                let by = [m.publisher.clone(), year.clone()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
                s += &format!(" ({by})");
                if !pages.is_empty() { s += &format!(", pp. {pages}"); }
                s += ".";
                return s;
            }
            if !m.journal.is_empty() {
                s += &format!(" *{}*", m.journal);
                if !m.volume.is_empty() { s += &format!(" **{}**", m.volume); }
                if !pages.is_empty() { s += &format!(", {pages}"); }
            }
            s += &format!(" ({year}).");
            s
        }
        // ACS: up to ten authors (then "et al."), separated by semicolons; year in bold
        CitationStyle::Acs => {
            let list: Vec<String> = names.iter().take(10).map(|n| fam_init(n, true)).collect();
            let authors = if names.len() > 10 { format!("{}; et al.", list.join("; ")) } else { list.join("; ") };
            let mut s = if authors.is_empty() { format!("{title}.") } else { format!("{authors} {title}.") };
            if book && !m.journal.is_empty() {
                s += &format!(" In *{}*;", m.journal);
                if !m.publisher.is_empty() { s += &format!(" {},", m.publisher); }
                s += &format!(" {year}");
                if !pages.is_empty() { s += &format!("; pp {pages}"); }
                s += ".";
            } else if !m.journal.is_empty() {
                s += &format!(" *{}* **{year}**", m.journal);
                if !m.volume.is_empty() {
                    s += &format!(", *{}*", m.volume);
                    if !m.issue.is_empty() { s += &format!(" ({})", m.issue); }
                }
                if !pages.is_empty() { s += &format!(", {pages}"); }
                s += ".";
            }
            if !m.doi.is_empty() { s += &format!(" DOI: {}.", m.doi); }
            s
        }
        CitationStyle::Apa | CitationStyle::Vancouver | CitationStyle::Nature => styled(p, style),
    }
}
