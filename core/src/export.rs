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
pub(crate) fn ascii_fold(s: &str) -> String {
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
    let s = s
        .replace('\\', "\\textbackslash{}")
        .replace('&', "\\&")
        .replace('%', "\\%")
        .replace('#', "\\#")
        .replace('_', "\\_")
        .replace('{', "\\{")
        .replace('}', "\\}");
    latex_letters(&s)
}

/// Accented and special Latin letters as LaTeX commands ("Paweł" → "Pawe{\l}", "Preguiça" →
/// "Pregui{\c{c}}a"), as BibTeX exports usually have them: classic BibTeX sorts and abbreviates
/// them right, and every engine and font prints them (Unicode "ł" is missing from the default
/// fonts of pdfLaTeX and XeTeX without fontspec). Other characters stay as they are.
/// LaTeX's accent commands, the letters they make and those letters' bases (in the same order).
const ACCENTS: [(&str, &str, &str); 13] = [
        ("`", "àèìòùÀÈÌÒÙ", "aeiouAEIOU"),
        ("'", "áéíóúýćńśźĺŕÁÉÍÓÚÝĆŃŚŹĹŔ", "aeiouycnszlrAEIOUYCNSZLR"),
        ("^", "âêîôûĉĝĥĵŝŵŷÂÊÎÔÛĈĜĤĴŜŴŶ", "aeioucghjswyAEIOUCGHJSWY"),
        ("~", "ãñõĩũÃÑÕĨŨ", "anoiuANOIU"),
        ("\"", "äëïöüÿÄËÏÖÜŸ", "aeiouyAEIOUY"),
        ("r", "åůÅŮ", "auAU"),
        ("c", "çşţģķļņÇŞŢĢĶĻŅ", "cstgklnCSTGKLN"),
        ("v", "čďěňřšťžČĎĚŇŘŠŤŽ", "cdenrstzCDENRSTZ"),
        ("u", "ăğŭĂĞŬ", "aguAGU"),
        ("H", "őűŐŰ", "ouOU"),
        ("k", "ąęįųĄĘĮŲ", "aeiuAEIU"),
        (".", "żėŻĖİ", "zezEI"),
        ("=", "āēīōūĀĒĪŌŪ", "aeiouAEIOU"),
    ];

/// The letter a base and a combining accent make ('C' + U+030C → 'Č'), for LaTeX read back.
pub(crate) fn compose(base: char, mark: char) -> Option<char> {
    let cmd = match mark {
        '\u{0300}' => "`", '\u{0301}' => "'", '\u{0302}' => "^", '\u{0303}' => "~", '\u{0308}' => "\"",
        '\u{030A}' => "r", '\u{0327}' => "c", '\u{030C}' => "v", '\u{0306}' => "u", '\u{030B}' => "H",
        '\u{0328}' => "k", '\u{0307}' => ".", '\u{0304}' => "=",
        _ => return None,
    };
    let (_, letters, bases) = ACCENTS.iter().find(|(c, _, _)| *c == cmd)?;
    bases.chars().position(|b| b == base).and_then(|i| letters.chars().nth(i))
}

pub fn latex_letters(s: &str) -> String {
    const SPECIAL: [(char, &str); 13] = [
        ('ß', "{\\ss}"), ('æ', "{\\ae}"), ('Æ', "{\\AE}"), ('ø', "{\\o}"), ('Ø', "{\\O}"), ('œ', "{\\oe}"), ('Œ', "{\\OE}"),
        ('ł', "{\\l}"), ('Ł', "{\\L}"), ('ı', "{\\i}"), ('đ', "{\\dj}"), ('Đ', "{\\DJ}"), ('ð', "{\\dh}"),
    ];
    let mut out = String::with_capacity(s.len());
    'chars: for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        if let Some((_, r)) = SPECIAL.iter().find(|(x, _)| *x == c) {
            out.push_str(r);
            continue;
        }
        for (cmd, letters, bases) in ACCENTS {
            if let Some(i) = letters.chars().position(|x| x == c) {
                let base = bases.chars().nth(i).unwrap();
                // \i: a dotless i under the accent
                let base = if base == 'i' { "\\i".to_string() } else { base.to_string() };
                let letter_cmd = cmd.chars().all(|x| x.is_ascii_alphabetic());
                out.push_str(&if letter_cmd { format!("{{\\{cmd}{{{base}}}}}") } else { format!("{{\\{cmd}{base}}}") });
                continue 'chars;
            }
        }
        out.push(c);
    }
    out
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
        Preprint | Dataset | WebPage | BlogPost | Other => "misc",
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

/// How BibTeX outputs shorten long author lists: a paper with more than `max_authors` authors
/// gets its first `shown_authors` and "and others", which BibTeX and biblatex styles print as
/// "et al." (0: never shorten). A library-wide setting (`Library::set_bibtex_options`).
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BibtexOptions {
    pub max_authors: u32,
    pub shown_authors: u32,
}

impl Default for BibtexOptions {
    /// More than 15 authors: the first 10 and "and others".
    fn default() -> Self {
        BibtexOptions { max_authors: 15, shown_authors: 10 }
    }
}

impl BibtexOptions {
    /// Within bounds: at least one author shown, never more than the limit.
    pub fn checked(self) -> BibtexOptions {
        if self.max_authors == 0 {
            return BibtexOptions { max_authors: 0, shown_authors: self.shown_authors.max(1) };
        }
        BibtexOptions { max_authors: self.max_authors, shown_authors: self.shown_authors.clamp(1, self.max_authors) }
    }

    /// The `author` field for these authors (already in BibTeX's form).
    pub fn author_field(&self, authors: &[String]) -> String {
        let o = self.checked();
        if o.max_authors > 0 && authors.len() > o.max_authors as usize {
            let mut a: Vec<String> = authors[..o.shown_authors as usize].to_vec();
            a.push("others".into());
            a.join(" and ")
        } else {
            authors.join(" and ")
        }
    }
}

/// Words a title in title case may leave in lower case (prepositions and the like), which don't
/// tell it from sentence case.
const TITLE_SMALL: [&str; 24] = [
    "with", "from", "into", "onto", "over", "upon", "than", "that", "versus", "about", "after", "among", "within", "without",
    "along", "through", "between", "under", "across", "against", "toward", "towards", "during", "beyond",
];

/// A word's letters and digits without the punctuation around it ("(BRCA1)," → "BRCA1"), with
/// what was before and after.
fn word_core(w: &str) -> (&str, &str, &str) {
    let start = w.find(|c: char| c.is_alphanumeric()).unwrap_or(w.len());
    let end = w.rfind(|c: char| c.is_alphanumeric()).map(|i| i + w[i..].chars().next().unwrap().len_utf8()).unwrap_or(start);
    if start >= end {
        return (w, "", "");
    }
    (&w[..start], &w[start..end], &w[end..])
}

/// A word that must keep its capitals whatever the style does: an acronym or a word with
/// capitals inside (GROBID, AI, arXiv, iPhone, PDFFigures, OpenAlex) or capitals and digits
/// (BRCA1, Qwen3, S2ORC, 82M).
fn keeps_capitals(w: &str) -> bool {
    let letters = w.chars().filter(|c| c.is_alphabetic()).count();
    let upper_inside = w.chars().skip(1).any(|c| c.is_uppercase());
    let digits_and_capitals = w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(|c| c.is_uppercase());
    letters > 0 && (upper_inside || digits_and_capitals)
}

/// Whether a title is in sentence case ("Robust annotation positioning in digital documents"):
/// at least as many longer words in lower case as capitalized ones, leaving out the first word,
/// the first after a colon, acronyms and title case's small words. In sentence case a capital
/// marks a proper noun ("Bayesian", "Markov"), which is then kept.
fn sentence_case(title: &str) -> bool {
    let (mut lower, mut upper) = (0, 0);
    let mut start = true;
    for token in title.split_whitespace() {
        for (i, part) in token.split('-').enumerate() {
            let (_, w, _) = word_core(part);
            let first = start && i == 0;
            if !first && w.chars().count() >= 4 && !keeps_capitals(w) && !TITLE_SMALL.contains(&w.to_lowercase().as_str()) {
                match w.chars().next() {
                    Some(c) if c.is_lowercase() => lower += 1,
                    Some(c) if c.is_uppercase() => upper += 1,
                    _ => {}
                }
            }
        }
        start = token.ends_with([':', '.', '?', '!']);
    }
    lower >= 1 && lower >= upper
}

/// A title for BibTeX: escaped, with braces only around what must stay as written, so the
/// bibliography style decides the case of the rest (most styles that change case put titles in
/// sentence case). Kept: acronyms and words with capitals inside or with digits (`{GROBID}`,
/// `{arXiv}`, `{iPhone}`, `{BRCA1}`, `{RNA}-seq`), and in a sentence-case title the capitalized
/// words after its start (proper nouns: `{Bayesian}`, `{Markov}`). A title in title case keeps
/// its other words unbraced: a style that lowercases titles lowercases them.
pub fn bib_title(title: &str) -> String {
    let sentence = sentence_case(title);
    let mut out: Vec<String> = vec![];
    let mut start = true;
    for token in title.split_whitespace() {
        let parts: Vec<String> = token
            .split('-')
            .enumerate()
            .map(|(i, part)| {
                let (before, w, after) = word_core(part);
                let first = start && i == 0;
                let proper = sentence && !first && w.chars().next().is_some_and(|c| c.is_uppercase());
                if !w.is_empty() && (keeps_capitals(w) || proper) {
                    format!("{}{{{}}}{}", bib_escape(before), bib_escape(w), bib_escape(after))
                } else {
                    bib_escape(part)
                }
            })
            .collect();
        out.push(parts.join("-"));
        start = token.ends_with([':', '.', '?', '!']);
    }
    out.join(" ")
}

/// BibTeX for papers outside the library (cited in a document only): the default options, no
/// fields of their own.
pub fn bibtex(papers: &[Paper]) -> String {
    bibtex_with(papers, BibtexOptions::default(), &std::collections::BTreeMap::new())
}

/// BibTeX for papers. `own`: paper id -> fields from an imported .bib, written as they are in
/// place of Lunta's field of the same name, or added (see `OpKind::SetBibFields`).
pub fn bibtex_with(papers: &[Paper], options: BibtexOptions, own: &std::collections::BTreeMap<String, Vec<(String, String)>>) -> String {
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
        let mut fields: Vec<(String, String)> = vec![];
        let mut field = |k: &str, v: &str| {
            if !v.trim().is_empty() {
                fields.push((k.to_string(), v.to_string()));
            }
        };
        field("title", &bib_title(&m.title));
        let authors: Vec<String> = m.authors.iter().map(|a| bib_name(a)).collect();
        field("author", &options.author_field(&authors));
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
                // a web page: plain BibTeX styles print howpublished and note (not url or
                // urldate), biblatex prints url and urldate: both get the address and the day
                Kind::WebPage | Kind::BlogPost => {
                    let url = bib_url(&m.url);
                    let site = if m.journal.is_empty() { String::new() } else { format!("{journal}, ") };
                    field("howpublished", &if url.is_empty() { journal.clone() } else { format!("{site}\\url{{{url}}}") });
                }
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
        field("url", &bib_url(&m.url));
        if matches!(kind, Kind::WebPage | Kind::BlogPost) {
            // biblatex: the full date of the post, and urldate ("visited on …")
            if m.date.len() > 4 {
                field("date", &m.date);
            }
            field("urldate", &m.accessed);
            if !m.accessed.is_empty() {
                field("note", &format!("Accessed {}", m.accessed));
            }
        }
        // the researcher's own fields win
        for (k, v) in own.get(&p.id).map(|v| v.as_slice()).unwrap_or(&[]) {
            match fields.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(k)) {
                Some(f) => f.1 = v.clone(),
                None => fields.push((k.clone(), v.clone())),
            }
        }
        out += &format!("@{entry}{{{key},\n");
        for (k, v) in fields.iter().filter(|(_, v)| !v.trim().is_empty()) {
            out += &format!("  {k} = {{{v}}},\n");
        }
        out += "}\n\n";
    }
    out
}

/// An address in a BibTeX field: `%` and `#` escaped (they end a line / mark a macro parameter
/// even inside `\url`), braces kept balanced.
fn bib_url(u: &str) -> String {
    u.trim().replace('%', "\\%").replace('#', "\\#").replace(['{', '}'], "")
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
        WebPage => "ELEC",
        BlogPost => "BLOG",
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
            "RPRT" | "STAND" | "THES" | "DATA" | "GEN" | "ELEC" | "BLOG" => line("T2", &m.journal),
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
        // the day a web page was read (Zotero's and EndNote's "Accessed")
        line("Y2", &m.accessed.replace('-', "/"));
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
    if matches!(crate::worktype::kind(&p.meta), crate::worktype::Kind::WebPage | crate::worktype::Kind::BlogPost) {
        return web_citation(p, style);
    }
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

// ---- web pages and blog posts ----

const MONTH_NAMES: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// (year, month 1–12 or 0, day or 0) of an ISO date ("2019-04-25", "2019-04", "2019").
fn date_parts(d: &str) -> (String, usize, u32) {
    let mut it = d.split('-');
    let y = it.next().unwrap_or("").trim().to_string();
    let m = it.next().and_then(|x| x.parse::<usize>().ok()).filter(|m| (1..=12).contains(m)).unwrap_or(0);
    let day = if m > 0 { it.next().and_then(|x| x.parse::<u32>().ok()).filter(|d| (1..=31).contains(d)).unwrap_or(0) } else { 0 };
    (y, m, day)
}

/// A month's name shortened as a style does: `n` letters with a dot ("Apr."), except the months
/// short enough to stay whole (`keep`: "May", "June", "July"), and MLA's "Sept.".
fn month_short(m: usize, mla: bool) -> String {
    let name = MONTH_NAMES[m - 1];
    match (name, mla) {
        ("May", _) => "May".into(),
        ("June" | "July", true) => name.into(),
        ("September", true) => "Sept.".into(),
        _ => format!("{}.", &name[..3]),
    }
}

/// A date written out: "April 25, 2019" (`us`), "25 April 2019", or "2019" / "April 2019" when
/// that is all there is.
fn long_date(d: &str, us: bool) -> String {
    let (y, m, day) = date_parts(d);
    match (m, day) {
        (0, _) => y,
        (m, 0) => format!("{} {y}", MONTH_NAMES[m - 1]),
        (m, day) if us => format!("{} {day}, {y}", MONTH_NAMES[m - 1]),
        (m, day) => format!("{day} {} {y}", MONTH_NAMES[m - 1]),
    }
}

/// A web page or blog post in a built-in style: who, when it was posted, the title, the site,
/// the address and, where the style asks for it (or the page has no date), the day it was read.
/// APA 7: "Karpathy, A. (2019, April 25). A Recipe for Training Neural Networks. Andrej Karpathy
/// blog. https://karpathy.github.io/2019/04/25/recipe/" (a web page's title in italics, a blog
/// post's not; no author: the title first).
fn web_citation(p: &Paper, style: CitationStyle) -> String {
    let m = &p.meta;
    let blog = crate::worktype::kind(m) == crate::worktype::Kind::BlogPost;
    let names: Vec<(String, String)> = m.authors.iter().map(|a| split_name(a)).collect();
    let title = m.title.trim().trim_end_matches('.').to_string();
    let title = if title.is_empty() { m.url.clone() } else { title };
    // the site, unless it only repeats the author ("World Health Organization")
    let site = if m.authors.len() == 1 && m.authors[0].eq_ignore_ascii_case(m.journal.trim()) { String::new() } else { m.journal.trim().to_string() };
    let url = m.url.trim().to_string();
    let (year, month, day) = date_parts(&m.date);
    let accessed = m.accessed.trim();
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
    let year_or_nd = if year.is_empty() { "n.d.".to_string() } else { year.clone() };
    let join = |parts: Vec<String>, sep: &str| parts.into_iter().filter(|x| !x.trim().is_empty()).collect::<Vec<_>>().join(sep);

    match style {
        CitationStyle::Apa => {
            let fmt = |n: &(String, String)| fam_init(n, true);
            let authors = match names.len() {
                0 => String::new(),
                1 => fmt(&names[0]),
                n if n <= 20 => format!("{}, & {}", names[..n - 1].iter().map(fmt).collect::<Vec<_>>().join(", "), fmt(&names[n - 1])),
                n => format!("{}, … {}", names[..19].iter().map(fmt).collect::<Vec<_>>().join(", "), fmt(&names[n - 1])),
            };
            let when = match (month, day) {
                _ if year.is_empty() => "n.d.".to_string(),
                (0, _) => year.clone(),
                (m, 0) => format!("{year}, {}", MONTH_NAMES[m - 1]),
                (m, d) => format!("{year}, {} {d}", MONTH_NAMES[m - 1]),
            };
            let t = if blog { title.clone() } else { format!("*{title}*") };
            let mut s = if authors.is_empty() {
                format!("{t}. ({when}).")
            } else {
                let a = if authors.ends_with('.') { authors.clone() } else { format!("{authors}.") };
                format!("{a} ({when}). {t}.")
            };
            if !site.is_empty() {
                s += &format!(" {site}.");
            }
            // a page without a date can change: when it was read (APA's "Retrieved … from")
            if year.is_empty() && !accessed.is_empty() && !url.is_empty() {
                s += &format!(" Retrieved {}, from {url}", long_date(accessed, true));
            } else if !url.is_empty() {
                s += &format!(" {url}");
            }
            s
        }
        CitationStyle::Vancouver => {
            let list: Vec<String> = names.iter().take(6).map(|(f, g)| format!("{f} {}", initials(g, false, false)).trim().to_string()).collect();
            let authors = if names.len() > 6 { format!("{}, et al", list.join(", ")) } else { list.join(", ") };
            let mut s = if authors.is_empty() { String::new() } else { format!("{authors}. ") };
            s += &format!("{title} [Internet].");
            let when = join(vec![year.clone(), if month > 0 { MONTH_NAMES[month - 1][..3].to_string() } else { String::new() }, if day > 0 { day.to_string() } else { String::new() }], " ");
            let place = join(vec![site.clone(), when], "; ");
            if !place.is_empty() {
                s += &format!(" {place}");
            }
            if !accessed.is_empty() {
                let (ay, am, ad) = date_parts(accessed);
                s += &format!(" [cited {}]", join(vec![ay, if am > 0 { MONTH_NAMES[am - 1][..3].to_string() } else { String::new() }, if ad > 0 { ad.to_string() } else { String::new() }], " "));
            }
            s += ".";
            if !url.is_empty() {
                s += &format!(" Available from: {url}");
            }
            s
        }
        CitationStyle::Nature => {
            let fmt = |n: &(String, String)| fam_init(n, true);
            let authors = match names.len() {
                0 => String::new(),
                1 => fmt(&names[0]),
                n if n <= 5 => format!("{} & {}", names[..n - 1].iter().map(fmt).collect::<Vec<_>>().join(", "), fmt(&names[n - 1])),
                _ => format!("{} et al.", fmt(&names[0])),
            };
            let mut s = if authors.is_empty() { format!("{title}.") } else { format!("{authors} {title}.") };
            if !site.is_empty() {
                s += &format!(" *{site}*");
            }
            if !url.is_empty() {
                s += &format!(" {url}");
            }
            s += &format!(" ({}).", if year.is_empty() { accessed.chars().take(4).collect::<String>() } else { year.clone() });
            s.replace(" ().", ".")
        }
        // Harvard (Cite Them Right): "Available at: … (Accessed: 21 November 2026)."
        CitationStyle::Harvard => {
            let authors = if names.len() > 3 {
                format!("{} et al.", fam_init(&names[0], false))
            } else {
                and_list(&names.iter().map(|n| fam_init(n, false)).collect::<Vec<_>>(), "and", false)
            };
            let mut s = if authors.is_empty() { format!("*{title}* ({year_or_nd})") } else if blog { format!("{authors} ({year_or_nd}) ‘{title}’") } else { format!("{authors} ({year_or_nd}) *{title}*") };
            if !site.is_empty() {
                s += &if blog { format!(", *{site}*") } else { format!(". {site}") };
            }
            if month > 0 && day > 0 {
                s += &format!(", {day} {}", MONTH_NAMES[month - 1]);
            }
            s += ".";
            if !url.is_empty() {
                s += &format!(" Available at: {url}");
                if !accessed.is_empty() {
                    s += &format!(" (Accessed: {})", long_date(accessed, false));
                }
                s += ".";
            }
            s
        }
        // Chicago author-date: "Karpathy, Andrej. 2019. “Title.” *Site* (blog), April 25, 2019. URL."
        CitationStyle::Chicago => {
            let list: Vec<String> = names.iter().enumerate().map(|(i, n)| if i == 0 { full_inverted(n) } else { full(n) }).collect();
            let mut authors = if names.len() > 10 { format!("{}, et al", list[..7].join(", ")) } else { and_list(&list, "and", true) };
            if !authors.is_empty() && !authors.ends_with('.') {
                authors.push('.');
            }
            let mut s = if authors.is_empty() { format!("“{title}.” {year_or_nd}.") } else { format!("{authors} {year_or_nd}. “{title}.”") };
            if !site.is_empty() {
                s += &if blog { format!(" *{site}* (blog)") } else { format!(" {site}") };
            }
            if month > 0 {
                s += &format!("{} {}", if site.is_empty() { "" } else { "," }, long_date(&m.date, true));
            }
            if !s.ends_with('.') {
                s += ".";
            }
            if year.is_empty() && !accessed.is_empty() {
                s += &format!(" Accessed {}.", long_date(accessed, true));
            }
            if !url.is_empty() {
                s += &format!(" {url}.");
            }
            s
        }
        // MLA 9: "… *Site*, 25 Apr. 2019, karpathy.github.io/2019/04/25/recipe/. Accessed 21 Nov. 2026."
        CitationStyle::Mla => {
            let mut authors = match names.len() {
                0 => String::new(),
                1 => full_inverted(&names[0]),
                2 => format!("{}, and {}", full_inverted(&names[0]), full(&names[1])),
                _ => format!("{}, et al", full_inverted(&names[0])),
            };
            if !authors.is_empty() {
                if !authors.ends_with('.') {
                    authors.push('.');
                }
                authors.push(' ');
            }
            let mla_date = |d: &str| {
                let (y, mo, da) = date_parts(d);
                join(vec![if da > 0 { da.to_string() } else { String::new() }, if mo > 0 { month_short(mo, true) } else { String::new() }, y], " ")
            };
            let bare = url.trim_start_matches("https://").trim_start_matches("http://").to_string();
            let s = format!("{authors}“{title}.”");
            let rest = join(vec![if site.is_empty() { String::new() } else { format!("*{site}*") }, mla_date(&m.date), bare], ", ");
            let mut s = if rest.is_empty() { s } else { format!("{s} {rest}.") };
            if !accessed.is_empty() {
                s += &format!(" Accessed {}.", mla_date(accessed));
            }
            s
        }
        CitationStyle::Cell => {
            let list: Vec<String> = names.iter().take(10).map(|n| fam_init(n, false)).collect();
            let authors = if names.len() > 10 { format!("{}, et al.", list.join(", ")) } else { and_list(&list, "and", true) };
            let mut s = if authors.is_empty() { format!("{title} ({year_or_nd}).") } else { format!("{authors} ({year_or_nd}). {title}.") };
            if !site.is_empty() {
                s += &format!(" {site}.");
            }
            if !url.is_empty() {
                s += &format!(" {url}.");
            }
            s
        }
        // IEEE: "A. Karpathy, “Title,” Site. Accessed: Nov. 21, 2026. [Online]. Available: URL"
        CitationStyle::Ieee => {
            let authors = if names.len() > 6 { format!("{} *et al.*", init_fam(&names[0])) } else { and_list(&names.iter().map(init_fam).collect::<Vec<_>>(), "and", true) };
            let mut s = if authors.is_empty() { format!("“{title},”") } else { format!("{authors}, “{title},”") };
            let when = match (month, day) {
                (0, _) => year.clone(),
                (mo, 0) => format!("{} {year}", month_short(mo, false)),
                (mo, d) => format!("{} {d}, {year}", month_short(mo, false)),
            };
            s += &format!(" {}.", join(vec![site.clone(), when], ", "));
            let s = s.replace(" .", "").replace(",”.", ".”");
            let mut s = s;
            if !accessed.is_empty() {
                let (ay, am, ad) = date_parts(accessed);
                let a = match (am, ad) {
                    (0, _) => ay,
                    (mo, 0) => format!("{} {ay}", month_short(mo, false)),
                    (mo, d) => format!("{} {d}, {ay}", month_short(mo, false)),
                };
                s += &format!(" Accessed: {a}.");
            }
            if !url.is_empty() {
                s += &format!(" [Online]. Available: {url}");
            }
            s
        }
        // AMA: "Karpathy A. Title. Site. Published April 25, 2019. Accessed November 21, 2026. URL"
        CitationStyle::Ama => {
            let fmt = |(f, g): &(String, String)| format!("{f} {}", initials(g, false, false)).trim().to_string();
            let authors = if names.len() > 6 { format!("{}, et al", names.iter().take(3).map(fmt).collect::<Vec<_>>().join(", ")) } else { names.iter().map(fmt).collect::<Vec<_>>().join(", ") };
            let mut s = if authors.is_empty() { format!("{title}.") } else { format!("{authors}. {title}.") };
            if !site.is_empty() {
                s += &format!(" {site}.");
            }
            if !year.is_empty() {
                s += &format!(" Published {}.", long_date(&m.date, true));
            }
            if !accessed.is_empty() {
                s += &format!(" Accessed {}.", long_date(accessed, true));
            }
            if !url.is_empty() {
                s += &format!(" {url}");
            }
            s
        }
        CitationStyle::Science | CitationStyle::Pnas => {
            let authors = if names.len() > 5 { format!("{} et al.", init_fam(&names[0])) } else { names.iter().map(init_fam).collect::<Vec<_>>().join(", ") };
            let mut s = if authors.is_empty() { format!("{title}.") } else { format!("{authors}, {title}.") };
            if !site.is_empty() {
                s += &format!(" *{site}*");
            }
            s += &format!(" ({year_or_nd})");
            if !url.is_empty() {
                s += &format!("; {url}");
            }
            s += ".";
            s
        }
        // ACS: "Karpathy, A. Title. Site. URL (accessed 2026-11-21)."
        CitationStyle::Acs => {
            let list: Vec<String> = names.iter().take(10).map(|n| fam_init(n, true)).collect();
            let authors = if names.len() > 10 { format!("{}; et al.", list.join("; ")) } else { list.join("; ") };
            let mut s = if authors.is_empty() { format!("{title}.") } else { format!("{authors} {title}.") };
            if !site.is_empty() {
                s += &format!(" {site}.");
            }
            if !url.is_empty() {
                s += &format!(" {url}");
                if !accessed.is_empty() {
                    s += &format!(" (accessed {accessed})");
                }
                s += ".";
            }
            s
        }
    }
}
