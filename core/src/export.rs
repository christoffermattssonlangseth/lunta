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
        _ => more_citation(p, style),
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
fn more_citation(p: &Paper, style: CitationStyle) -> String {
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
            if !m.journal.is_empty() {
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
            if !m.journal.is_empty() {
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
            if !m.journal.is_empty() {
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
            if !m.journal.is_empty() {
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
            if !m.journal.is_empty() {
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
            if !m.journal.is_empty() {
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
        CitationStyle::Apa | CitationStyle::Vancouver | CitationStyle::Nature => citation(p, style),
    }
}
