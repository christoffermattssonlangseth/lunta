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

fn first_word(title: &str) -> String {
    let stop = ["a", "an", "the", "of", "on", "in", "for", "and", "to", "with"];
    title
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| !w.is_empty() && !stop.contains(&w.to_lowercase().as_str()))
        .unwrap_or("paper")
        .to_lowercase()
}

pub fn cite_key(m: &PaperMeta) -> String {
    let fam = m.authors.first().map(|a| split_name(a).0).unwrap_or_else(|| "anon".into());
    let fam: String = fam.chars().filter(|c| c.is_alphanumeric()).collect();
    format!("{}{}{}", fam, m.year(), first_word(&m.title))
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

pub fn bibtex(papers: &[Paper]) -> String {
    let mut used = std::collections::HashMap::<String, u32>::new();
    let mut out = String::new();
    for p in papers {
        let m = &p.meta;
        let mut key = cite_key(m);
        let n = used.entry(key.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            key.push((b'a' + (*n as u8 - 1)) as char);
        }
        let kind = if m.journal.is_empty() || m.arxiv == m.journal { "misc" } else { "article" };
        out += &format!("@{kind}{{{key},\n");
        let mut field = |k: &str, v: &str| {
            if !v.trim().is_empty() {
                out += &format!("  {k} = {{{v}}},\n");
            }
        };
        field("title", &format!("{{{}}}", bib_escape(&m.title)));
        let authors: Vec<String> = m.authors.iter().map(|a| {
            let (f, g) = split_name(a);
            if g.is_empty() { bib_escape(&f) } else { format!("{}, {}", bib_escape(&f), bib_escape(&g)) }
        }).collect();
        field("author", &authors.join(" and "));
        field("journal", &bib_escape(&m.journal));
        field("year", &m.year());
        field("volume", &m.volume);
        field("number", &m.issue);
        field("pages", &m.pages.replace('-', "--").replace('–', "--"));
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

pub fn ris(papers: &[Paper]) -> String {
    let mut out = String::new();
    for p in papers {
        let m = &p.meta;
        let mut line = |k: &str, v: &str| {
            if !v.trim().is_empty() {
                out += &format!("{k}  - {}\r\n", v.trim());
            }
        };
        line("TY", if m.journal.is_empty() { "GEN" } else { "JOUR" });
        for a in &m.authors {
            let (f, g) = split_name(a);
            line("AU", &if g.is_empty() { f } else { format!("{f}, {g}") });
        }
        line("TI", &m.title);
        line("JO", &m.journal);
        line("PY", &m.year());
        line("DA", &m.date.replace('-', "/"));
        line("VL", &m.volume);
        line("IS", &m.issue);
        let (sp, ep) = m.pages.split_once(['-', '–']).unwrap_or((m.pages.as_str(), ""));
        line("SP", sp);
        line("EP", ep);
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

pub fn citation(p: &Paper, style: CitationStyle) -> String {
    let m = &p.meta;
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
            if !m.journal.is_empty() {
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
            if !m.journal.is_empty() {
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
    }
}
