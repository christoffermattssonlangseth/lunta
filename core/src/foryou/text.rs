//! New for You's words: paper keys, the "why" of each source, the language model's prompts and
//! how its answers are read, and the digest's links. Ported from apple/Shared/Watches.swift
//! (WatchCenter's static functions), so every device writes the same things into foryou.json.

use super::state::FeedRecord;
use crate::metadata::DiscoverHit;
use regex::Regex;
use std::sync::OnceLock;

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).unwrap())
}

// ---- keys ----

/// A title as a key: lowercase letters and digits, at most 70 (LibraryModel.titleKey).
pub fn title_key(t: &str) -> String {
    t.to_lowercase().chars().filter(|c| c.is_alphanumeric()).take(70).collect()
}

/// A DOI as a key: lowercase, without "https://doi.org/" or "doi:" (LibraryModel.doiKey).
pub fn doi_key(d: &str) -> String {
    let mut s = d.trim_matches(|c: char| c == ' ' || c == '\t').to_lowercase();
    for p in ["https://doi.org/", "http://doi.org/", "https://dx.doi.org/", "doi:"] {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest.to_string();
        }
    }
    s
}

/// A found paper's key: its DOI, else its title (LibraryModel.hitKey).
pub fn hit_key(h: &DiscoverHit) -> String {
    if h.meta.doi.is_empty() {
        title_key(&h.meta.title)
    } else {
        doi_key(&h.meta.doi)
    }
}

/// Title and the start of the abstract: what rankings compare and feedback remembers.
pub fn snip(title: &str, abstract_text: &str) -> String {
    format!("{title}. {}", abstract_text.chars().take(400).collect::<String>())
}

// ---- sources ----

pub fn cites_your_paper(title: &str) -> String {
    let first = title.split(": ").next().unwrap_or(title);
    let short = if first.chars().count() >= 12 { first } else { title };
    let short = if short.chars().count() > 70 { format!("{}…", short.chars().take(68).collect::<String>()) } else { short.to_string() };
    format!("Cites your paper “{short}”.")
}

pub fn cites_yours(source: &str) -> bool {
    source.starts_with("Cites your paper “")
}

pub fn journal_source(name: &str) -> String {
    format!("New in {name}, a journal you follow.")
}

pub fn from_journal(source: &str) -> bool {
    source.starts_with("New in ") && source.ends_with("a journal you follow.")
}

/// bioRxiv, medRxiv, arXiv and other preprint servers.
pub fn is_preprint(h: &DiscoverHit, source: &str) -> bool {
    let venue = h.meta.journal.to_lowercase();
    let doi = &h.meta.doi;
    source.ends_with("Rxiv")
        || venue.contains("rxiv")
        || venue.contains("preprint")
        || venue.contains("research square")
        || venue.contains("ssrn")
        || venue.contains("qeios")
        || ["10.1101/", "10.64898/", "10.48550/", "10.21203/"].iter().any(|p| doi.starts_with(p))
}

/// The record's paper as a found paper (FeedRecord.hit).
pub fn record_hit(r: &FeedRecord) -> DiscoverHit {
    DiscoverHit {
        meta: crate::model::PaperMeta {
            title: r.title.clone(),
            authors: r.authors.clone(),
            date: r.date.clone(),
            journal: r.journal.clone(),
            doi: r.doi.clone(),
            url: r.url.clone(),
            abstract_text: r.r#abstract.clone(),
            ..Default::default()
        },
        cited_by: r.cited_by.max(0) as u32,
        open_access: r.open_access,
        in_library: r.in_library.clone(),
    }
}

/// "Jane Doe et al. · Nature" (FeedRow.byline).
pub fn byline(r: &FeedRecord) -> String {
    let who = match r.authors.first() {
        Some(a) if r.authors.len() > 1 => format!("{a} et al."),
        Some(a) => a.clone(),
        None => String::new(),
    };
    [who, r.journal.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// The first sentences of an abstract, up to about `max` characters: what a paper shows when no
/// model has summarized it.
pub fn first_sentences(abstract_text: &str, max: usize) -> String {
    let text = abstract_text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let mut out = String::new();
    for sentence in text.split_inclusive(". ") {
        if !out.is_empty() && out.chars().count() + sentence.chars().count() > max {
            break;
        }
        out.push_str(sentence);
    }
    if out.chars().count() > max + max / 2 {
        out = format!("{}…", out.chars().take(max).collect::<String>().trim_end());
    }
    out.trim_end().to_string()
}

// ---- the language model ----

pub const SEARCHES_INSTRUCTIONS: &str = "You help a researcher find new papers. Answer only in the requested format.";

pub fn searches_prompt(interests: &str) -> String {
    format!(
        "A researcher describes their interests:\n\n{interests}\n\n\
        Write 6 queries for a scholarly search engine that together cover these interests, each 3 to 6 words, \
        one per line, each line starting with \"Q: \". Each query covers a different part of the interests (together \
        they cover all of it, including the diseases and systems studied). Every query must name the researcher's field or kind of \
        data (for example \"spatial transcriptomics cell segmentation\", not \"cell segmentation\"), so that it finds \
        papers from their field rather than general ones. Then one line starting with \"K: \" with 12 to 20 \
        specific technical terms from their field (methods, data types, tools), separated by commas; no generic \
        words like \"analysis\", \"data\" or \"AI\"."
    )
}

/// Trims what models put around a query or keyword: quotes, dashes, bullets, numbering.
pub fn clean(s: &str) -> String {
    s.trim_matches(|c: char| " \t\"'“”-*•0123456789.".contains(c)).to_string()
}

/// The queries ("Q: …") and keywords ("K: …, …") of the model's answer; None when it gave
/// fewer than two distinct queries.
pub fn parse_searches(text: &str) -> Option<(Vec<String>, Vec<String>)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let q: Vec<String> = lines
        .iter()
        .filter(|l| l.to_uppercase().starts_with("Q:"))
        .map(|l| clean(&l[2..]))
        .filter(|q| q.chars().count() > 3)
        .collect();
    let k: Vec<String> = lines
        .iter()
        .find(|l| l.to_uppercase().starts_with("K:"))
        .map(|l| l[2..].split(',').map(|x| clean(x).to_lowercase()).filter(|x| x.chars().count() > 3).collect())
        .unwrap_or_default();
    // drop near-repeats ("x y" when "x y z" is there, or the other way round)
    let mut distinct: Vec<String> = vec![];
    for query in q {
        let words: std::collections::HashSet<String> = query.to_lowercase().split(' ').map(str::to_string).collect();
        let repeat = distinct.iter().any(|o| {
            let o: std::collections::HashSet<String> = o.to_lowercase().split(' ').map(str::to_string).collect();
            o.is_subset(&words) || words.is_subset(&o)
        });
        if !repeat {
            distinct.push(query);
        }
    }
    (distinct.len() >= 2).then(|| (distinct.into_iter().take(8).collect(), k.into_iter().take(24).collect()))
}

/// Without a model: the paragraph's own phrases as queries, its long words as keywords.
pub fn fallback_searches(interests: &str) -> (Vec<String>, Vec<String>) {
    let phrases: Vec<String> = interests
        .split(|c: char| ".,;:\n()".contains(c))
        .map(clean)
        .filter(|p| (2..=6).contains(&p.split(' ').filter(|w| !w.is_empty()).count()))
        .take(6)
        .collect();
    let mut words: Vec<String> = vec![];
    for w in interests.to_lowercase().split(|c: char| !c.is_alphabetic() && c != '-') {
        if w.chars().count() > 6 && !words.iter().any(|x| x == w) {
            words.push(w.to_string());
        }
    }
    words.truncate(20);
    (phrases, words)
}

pub const SCREEN_INSTRUCTIONS: &str = "You screen new scientific papers for one researcher. You are strict: most papers score below 7.";

pub fn screen_prompt(h: &DiscoverHit, interests: &str, liked: &[String]) -> String {
    let abstract_text = if h.meta.abstract_text.is_empty() {
        "(no abstract)".to_string()
    } else {
        h.meta.abstract_text.chars().take(1100).collect()
    };
    let examples = if liked.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nRecent papers they marked as exactly what they want:\n{}",
            liked.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n")
        )
    };
    format!(
        "The researcher's interests:\n{interests}{examples}\n\n\
        New paper:\nTitle: {}\nAbstract: {abstract_text}\n\n\
        Rate from 0 to 10 how well this paper matches the researcher's interests:\n\
        10 = squarely in their field and directly useful to their work;\n\
        5 = the same broad area but a different topic, organism, disease or question;\n\
        0 = unrelated.\n\
        Sharing only general words (such as \"multi-omics\", \"AI\", \"tissue\" or \"integration\") is worth at most 4.\n\n\
        Reply with a single line: SCORE: <number> — <one sentence of at most 20 words on what specifically \
        connects this paper to their work, addressed to them as \"you\"; \
        do not start with \"This paper\" — start with the specific point, e.g. \"A segmentation method for Xenium data, …\">",
        h.meta.title
    )
}

/// "SCORE: 8 — …" (tolerating "Score 8/10 - …") → (8, the sentence).
pub fn parse_screen(answer: &str) -> Option<(f64, String)> {
    let a = answer.trim();
    if a.is_empty() {
        return None;
    }
    let digits: String = a.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let score: f64 = digits.parse().ok()?;
    let mut why = a.to_string();
    for sep in ["—", " – ", " - "] {
        if let Some(i) = a.find(sep) {
            why = a[i + sep.len()..].to_string();
            break;
        }
    }
    let why = why.trim().trim_matches(|c: char| c == '"' || c == '*' || c.is_whitespace()).to_string();
    let why = if why.to_uppercase().starts_with("SCORE") || why.chars().count() < 8 { "Matches your interests.".to_string() } else { why };
    Some((score, why))
}

pub const SUMMARY_INSTRUCTIONS: &str = "You summarize scientific papers in one accurate sentence, using only what the abstract says.";

pub fn summary_prompt(title: &str, abstract_text: &str) -> String {
    format!(
        "Title: {title}\nAbstract: {}\n\n\
        In one plain sentence of at most 25 words, say what this paper does and what it finds. \
        Start with the method or finding itself (not \"This paper\" or \"The authors\"). No preamble.",
        abstract_text.chars().take(1500).collect::<String>()
    )
}

/// The model's sentence: trimmed of quotes and asterisks, its first line; None if too short.
pub fn clean_summary(answer: &str) -> Option<String> {
    let a = answer.trim_matches(|c: char| c.is_whitespace() || c == '"' || c == '*');
    if a.chars().count() <= 15 {
        return None;
    }
    let line = a.lines().find(|l| !l.trim().is_empty()).unwrap_or(a);
    Some(line.to_string())
}

pub const DIGEST_INSTRUCTIONS: &str = "You write short, concrete research digests. You never invent findings that aren't in the summaries.";

pub fn digest_prompt(interests: &str, batch: &[FeedRecord]) -> String {
    let list = batch
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let line = r.summary.clone().unwrap_or_else(|| r.r#abstract.chars().take(300).collect());
            format!("{}. {}\n   {line}", i + 1, r.title)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "The researcher's interests:\n{interests}\n\nThe papers in their feed:\n{list}\n\n\
        Summarize this feed for the researcher in ONE short paragraph of two or three sentences, \
        at most 70 words: the main themes across the papers, then the one or two most worth their time \
        and why. Each time you mention a paper, write it as a link: a short form of its title in square \
        brackets followed by its number in parentheses, e.g. [scPolyA](2). No list, no heading, no preamble."
    )
}

/// The model's digest as one paragraph, linked to the papers.
pub fn clean_digest(answer: &str, batch: &[FeedRecord]) -> Option<String> {
    let a = answer.trim();
    if a.chars().count() <= 20 {
        return None;
    }
    let one = a.replace("**", "").lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join(" ");
    Some(link_digest(&one, batch))
}

pub const DRAFT_INSTRUCTIONS: &str = "You describe a researcher's interests precisely and briefly.";

/// The prompt for drafting the paragraph from your own papers (titles; the newest 8 with the
/// start of their abstracts) and the papers you saved recently.
pub fn draft_prompt(own: &[(String, String)], saved: &[String]) -> String {
    let mut parts = vec![];
    if !own.is_empty() {
        let mine = own
            .iter()
            .enumerate()
            .map(|(i, (t, a))| {
                let abs = if i < 8 && !a.is_empty() { format!("\n  {}", a.chars().take(350).collect::<String>()) } else { String::new() };
                format!("- {t}{abs}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!("Papers the researcher wrote themselves (newest first; these matter most):\n\n{mine}"));
    }
    if !saved.is_empty() {
        let list = saved.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n");
        parts.push(format!("Papers the researcher saved recently:\n\n{list}"));
    }
    format!(
        "{}\n\nWrite their research interests as one paragraph of 3 to 5 sentences in the first person (\"I work on…\"). \
        Describe the field, the questions, the methods and the kinds of data in your own words, as a researcher \
        would describe their work. Be concrete: name the specific techniques (for example \"spatial transcriptomics\", \
        \"cell segmentation\"), tissues, organisms and diseases that recur in the papers; never use vague phrases \
        like \"advanced techniques\" or \"complex biological data\", never copy whole titles. Only the paragraph.",
        parts.join("\n\n")
    )
}

// ---- the digest's links ----

/// The link a digest uses for a paper in the feed ("foryou://paper?key=10%2E1101%2F…").
pub fn feed_link(key: &str) -> String {
    let mut out = String::from("foryou://paper?key=");
    for c in key.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// The paper key of a digest link, None for any other link.
pub fn feed_key(url: &str) -> Option<String> {
    let rest = url.strip_prefix("foryou://paper?")?;
    let v = rest.split('&').find_map(|kv| kv.strip_prefix("key="))?;
    let bytes = v.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = v.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn is_inside_link(s: &str, at: usize) -> bool {
    let before = &s[..at];
    before.matches('[').count() > before.matches(']').count() || (before.ends_with('(') && before[..before.len() - 1].ends_with(']'))
}

/// Turns the model's "[short title](3)" references into links to the papers, and links the
/// short names of papers it mentioned without a link (WatchCenter.linkDigest).
pub fn link_digest(text: &str, batch: &[FeedRecord]) -> String {
    static NUMBERED: OnceLock<Regex> = OnceLock::new();
    static NAMED: OnceLock<Regex> = OnceLock::new();
    static STRAY: OnceLock<Regex> = OnceLock::new();
    // numbered citations, "cell-type inference [1](1)": a small ↗ link after the phrase
    let out = re(&NUMBERED, r"(?i)\s?\[(?:paper\s)?(\d{1,2})\]\((\d{1,2})\)").replace_all(text, " [↗]($2)").to_string();
    // [name](n) → [name](foryou://…)
    let out = re(&NAMED, r"\[([^\]]+)\]\((\d+)\)")
        .replace_all(&out, |c: &regex::Captures| {
            let name = &c[1];
            match c[2].parse::<usize>() {
                Ok(n) if n >= 1 && n <= batch.len() => format!("[{name}]({})", feed_link(&batch[n - 1].key)),
                _ if name == "↗" => String::new(),
                _ => name.to_string(),
            }
        })
        .to_string();
    // stray "(3)" or "[3]" references
    let mut out = re(&STRAY, r"(?i)\s?[\(\[](paper\s)?\d{1,2}[\)\]]").replace_all(&out, "").to_string();
    // unlinked short names
    for r in batch {
        if out.contains(&feed_link(&r.key)) || !r.title.contains(':') {
            continue;
        }
        let short = r.title.split(':').next().unwrap_or("").trim();
        let n = short.chars().count();
        if !(3..=30).contains(&n) {
            continue;
        }
        if let Some(at) = out.find(short) {
            if !is_inside_link(&out, at) {
                out.replace_range(at..at + short.len(), &format!("[{short}]({})", feed_link(&r.key)));
            }
        }
    }
    link_bracketed(&repair_digest(&out), batch)
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// "[Title]" with no link after it: a link to the paper when it names one in the batch,
/// otherwise just the text (WatchCenter.linkBracketed).
pub fn link_bracketed(text: &str, batch: &[FeedRecord]) -> String {
    static BRACKETED: OnceLock<Regex> = OnceLock::new();
    let re = re(&BRACKETED, r"\[([^\[\]]+)\]");
    let mut out = String::new();
    let mut last = 0;
    for c in re.captures_iter(text) {
        let m = c.get(0).unwrap();
        if text[m.end()..].starts_with('(') {
            continue;
        }
        out.push_str(&text[last..m.start()]);
        let name = &c[1];
        let n = norm(name);
        let hit = (n.chars().count() >= 3)
            .then(|| {
                batch.iter().find(|r| {
                    let t = norm(&r.title);
                    t == n || t.starts_with(&n) || norm(r.title.split(':').next().unwrap_or("")) == n
                })
            })
            .flatten();
        match hit {
            Some(r) => out.push_str(&format!("[{name}]({})", feed_link(&r.key))),
            None => out.push_str(name),
        }
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

/// A bare "(foryou://…)" left without its link text: a ↗ link instead (WatchCenter.repairDigest).
pub fn repair_digest(text: &str) -> String {
    static BARE: OnceLock<Regex> = OnceLock::new();
    let re = re(&BARE, r"\((foryou://[^)\s]+)\)");
    let mut out = String::new();
    let mut last = 0;
    for c in re.captures_iter(text) {
        let m = c.get(0).unwrap();
        let before = &text[..m.start()];
        if before.ends_with(']') {
            continue; // a link's target
        }
        // a space before it goes too, unless it follows a link
        let ws = before.chars().last().filter(|c| c.is_whitespace());
        let start = match ws {
            Some(w) if !before[..before.len() - w.len_utf8()].ends_with(']') => m.start() - w.len_utf8(),
            _ => m.start(),
        };
        out.push_str(&text[last..start]);
        out.push_str(&format!(" [↗]({})", &c[1]));
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

/// A piece of the digest: text, or a link to a paper in the feed (`key`).
#[derive(Clone, Debug, PartialEq)]
pub struct DigestPiece {
    pub text: String,
    pub key: Option<String>,
}

/// The digest in pieces, ready to draw: plain text and links to papers (the apps render
/// foryou.json's Markdown links this way).
pub fn digest_pieces(digest: &str, batch: &[FeedRecord]) -> Vec<DigestPiece> {
    static LINK: OnceLock<Regex> = OnceLock::new();
    let text = link_bracketed(&repair_digest(digest), batch);
    let re = re(&LINK, r"\[([^\]]+)\]\(([^)\s]*)\)");
    let mut out = vec![];
    let mut last = 0;
    let mut push = |t: &str, key: Option<String>| {
        if !t.is_empty() {
            out.push(DigestPiece { text: t.to_string(), key });
        }
    };
    for c in re.captures_iter(&text) {
        let m = c.get(0).unwrap();
        push(&text[last..m.start()], None);
        match feed_key(&c[2]) {
            Some(k) => push(&c[1], Some(k)),
            None => push(&c[1], None),
        }
        last = m.end();
    }
    push(&text[last..], None);
    out
}

/// The digest as plain text to send someone, followed by its papers with their links.
pub fn share_text(digest: &str, papers: &[FeedRecord]) -> String {
    static ARROW: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    let text = link_bracketed(&repair_digest(digest), papers);
    let text = re(&ARROW, r"\s?\[↗\]\([^)]*\)").replace_all(&text, "");
    let mut text = re(&LINK, r"\[([^\]]+)\]\([^)]*\)").replace_all(&text, "$1").to_string();
    let list: Vec<String> = papers
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let link = if !r.doi.is_empty() { format!("https://doi.org/{}", r.doi) } else { r.url.clone() };
            format!("{}. {}{}", i + 1, r.title, if link.is_empty() { String::new() } else { format!("\n   {link}") })
        })
        .collect();
    if !list.is_empty() {
        text += &format!("\n\nPapers:\n{}", list.join("\n"));
    }
    format!("New for You (Lunta)\n\n{text}")
}
