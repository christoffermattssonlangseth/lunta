//! Turns user input (link, DOI, PMCID, PubMed ID, arXiv id, file) into candidate sources,
//! best first. Mirrors `Resolver.swift`.

use super::net::{self, sniff};
use super::xml::Doc;
use super::{arxiv, fail, Options, PaperMeta, Result};
use regex::{Regex, RegexBuilder};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::OnceLock;

/// Where figure images for a JATS document can be found.
#[derive(Debug, Clone)]
pub enum AssetLocator {
    /// PMC open-data bucket on AWS: one folder per article version with the XML and every figure.
    S3 { base: String, files: Vec<String> },
    /// URL templates; `{href}` and `{stem}` are replaced with the graphic reference.
    Patterns(Vec<String>),
    None,
}

pub enum Source {
    Jats { data: Vec<u8>, assets: AssetLocator, meta: PaperMeta, label: String },
    Pdf { data: Vec<u8>, meta: PaperMeta, label: String },
    /// LaTeXML HTML (arXiv / ar5iv); image paths are relative to `base`.
    Html { doc: Doc, base: url::Url, meta: PaperMeta, label: String },
}

pub type Loader = Box<dyn FnOnce(&Options) -> Result<Source> + Send>;

pub struct Candidate {
    pub label: String,
    pub load: Loader,
}

pub fn cand(label: impl Into<String>, f: impl FnOnce(&Options) -> Result<Source> + Send + 'static) -> Candidate {
    Candidate { label: label.into(), load: Box::new(f) }
}

fn re(p: &str, ci: bool) -> Regex {
    RegexBuilder::new(p).case_insensitive(ci).build().unwrap()
}

/// First match of `pattern` in `s`: capture group 1 if the pattern has one, else the whole match.
pub fn first_match(pattern: &str, s: &str, ci: bool) -> Option<String> {
    let r = re(pattern, ci);
    let c = r.captures(s)?;
    let m = if c.len() > 1 { c.get(1) } else { c.get(0) };
    m.map(|m| m.as_str().to_string())
}

pub fn candidates(input: &str) -> Result<Vec<Candidate>> {
    let mut s = input.trim().to_string();
    if s.is_empty() {
        return Err(fail("Paste a link, DOI or PMCID first."));
    }
    if s.to_lowercase().starts_with("doi:") {
        s = super::trim_ws(&s[4..]).to_string();
    }

    // arXiv: links, IDs and arXiv DOIs
    if let Some(aid) = arxiv_id(&s) {
        return Ok(arxiv::candidates(&aid));
    }

    // PMCID, bare or inside a PMC / Europe PMC link
    if let Some(pmc) = first_match(r"\b(PMC\d{4,})\b", &s, true).map(|x| x.to_uppercase()) {
        let p = pmc.clone();
        return Ok(vec![cand(format!("PMC {pmc}"), move |o| pmc_source(&p, o))]);
    }

    // PubMed ID links
    if let Some(pmid) =
        first_match(r"(?:pubmed\.ncbi\.nlm\.nih\.gov/|europepmc\.org/(?:abstract|article)/MED/)(\d+)", &s, false)
    {
        let p = pmid.clone();
        return Ok(vec![cand(format!("PubMed {pmid}"), move |o| {
            europe_pmc_by_query(&format!("EXT_ID:{p} AND SRC:MED"), &format!("PMID {p}"), o)
        })]);
    }

    // DOI: doi.org links, bare DOIs, or a DOI embedded in a publisher URL
    if let Some(d) = doi_in(&s) {
        return Ok(doi_candidates(&d));
    }

    if s.starts_with("http://") || s.starts_with("https://") {
        if let Ok(u) = url::Url::parse(&s) {
            return Ok(vec![cand("web page", move |o| from_web_page(&u, o))]);
        }
    }
    if s.chars().all(|c| c.is_numeric()) && s.chars().count() >= 5 {
        let p = s.clone();
        return Ok(vec![cand(format!("PubMed {s}"), move |o| {
            europe_pmc_by_query(&format!("EXT_ID:{p} AND SRC:MED"), &format!("PMID {p}"), o)
        })]);
    }
    Err(fail(format!("Don't know how to read “{input}”. Try a DOI, a link to the article, or a PMCID.")))
}

/// Candidates for a file (PDF or JATS XML).
pub fn candidates_for_file(path: &Path) -> Result<Vec<Candidate>> {
    let data = std::fs::read(path)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match sniff(&data) {
        "pdf" => Ok(vec![cand(format!("PDF {name}"), move |_| {
            Ok(Source::Pdf { data, meta: PaperMeta::default(), label: "PDF file".into() })
        })]),
        "jats" => Ok(vec![cand(format!("JATS {name}"), move |_| {
            Ok(Source::Jats { data, assets: AssetLocator::None, meta: PaperMeta::default(), label: "JATS file".into() })
        })]),
        _ => Err(fail(format!("{name} is not a PDF or JATS XML file."))),
    }
}

// MARK: - Input parsing

pub fn arxiv_id(s: &str) -> Option<String> {
    let mut t = s.to_string();
    if let Some(m) = first_match(r"arxiv\.org/(?:abs|pdf|html)/([^\s?#]+?)(?:\.pdf)?/?$", s, true) {
        t = m;
    }
    t = re(r"^10\.48550/arxiv\.", true).replace(&t, "").into_owned();
    t = re(r"^https?://doi\.org/10\.48550/arxiv\.", true).replace(&t, "").into_owned();
    static AX: OnceLock<Regex> = OnceLock::new();
    let ax = AX.get_or_init(|| {
        re(r"^(?:arxiv:)?(\d{4}\.\d{4,5}(?:v\d+)?|[a-z\-]+(?:\.[A-Z]{2})?/\d{7}(?:v\d+)?)$", true)
    });
    ax.captures(&t).map(|c| c[1].to_string())
}

pub fn doi_in(s: &str) -> Option<String> {
    let decoded = net::percent_decode(s).unwrap_or_else(|| s.to_string());
    static DOI: OnceLock<Regex> = OnceLock::new();
    let r = DOI.get_or_init(|| re(r#"(10\.\d{4,9}/[^\s"<>?#]+)"#, true));
    let mut d = r.captures(&decoded)?[1].to_string();
    // strip publisher URL suffixes that follow the DOI
    for suffix in
        ["/full", "/abstract", "/pdf", "/epdf", "/pdfdirect", "/fulltext", ".full.pdf", ".full", ".pdf", ".abstract", "/"]
    {
        if d.to_lowercase().ends_with(suffix) {
            d.truncate(d.len() - suffix.len());
        }
    }
    d = d.trim_matches(|c| ".,;)".contains(c)).to_string();
    // bioRxiv / medRxiv URLs carry a version: 10.1101/2024.01.02.573123v2 -> 10.1101/2024.01.02.573123
    if d.starts_with("10.1101/") {
        d = re(r"v\d+$", false).replace(&d, "").into_owned();
    }
    Some(d)
}

/// A DOI recorded in a PDF's metadata (XMP `prism:doi` / `dc:identifier`, or an Info `/doi`).
/// Stand-in for Swift's `PDFAnalyzer.quickDOI`, which reads the page text with PDFKit.
pub fn doi_in_pdf_metadata(pdf: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(pdf);
    for p in [
        r"<prism:doi>\s*([^<\s]+)\s*</prism:doi>",
        r"<pdfx:doi>\s*([^<\s]+)\s*</pdfx:doi>",
        r#"prism:doi="([^"]+)""#,
        r"<dc:identifier>\s*(?:<rdf:Seq>\s*<rdf:li>)?\s*(?:doi:)?(10\.[^<\s]+)",
        r"/(?:doi|DOI)\s*\((10\.[^)]+)\)",
    ] {
        if let Some(m) = first_match(p, &s, false) {
            if let Some(d) = doi_in(&m) {
                return Some(d);
            }
        }
    }
    None
}

// MARK: - DOI

pub fn doi_candidates(doi: &str) -> Vec<Candidate> {
    if doi.to_lowercase().starts_with("10.48550/arxiv.") {
        return arxiv::candidates(&doi["10.48550/arxiv.".len()..]);
    }
    let mut out = Vec::new();
    let d = doi.to_string();
    out.push(cand(format!("Europe PMC full text for {doi}"), move |o| {
        europe_pmc_by_query(&format!("DOI:\"{d}\""), &d, o)
    }));
    if doi.to_lowercase().starts_with("10.7554/elife.") {
        let num: String = doi.split('.').next_back().unwrap_or("").chars().filter(|c| c.is_numeric()).collect();
        let n = num.clone();
        out.push(cand(format!("eLife XML {num}"), move |o| elife_source(&n, o)));
    }
    out
}

// MARK: - Europe PMC / PMC

pub fn europe_pmc_by_query(query: &str, what: &str, o: &Options) -> Result<Source> {
    let url = format!(
        "https://www.ebi.ac.uk/europepmc/webservices/rest/search?query={}&resultType=core&format=json",
        net::encode_query(query)
    );
    o.log(&format!("Looking up {what} in Europe PMC"));
    let r = net::with_retry(2, || net::json(&url))?;
    let results = r
        .get("resultList")
        .and_then(|x| x.get("result"))
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let Some(hit) = results.first() else { return Err(fail(format!("Europe PMC does not know {what}"))) };
    let meta = epmc_meta(hit);
    if let Some(pmcid) = hit.get("pmcid").and_then(|x| x.as_str()) {
        let mut src = pmc_source(pmcid, o)?;
        if let Source::Jats { meta: m, .. } = &mut src {
            m.merge(&meta);
        }
        return Ok(src);
    }
    if hit.get("source").and_then(|x| x.as_str()) == Some("PPR") {
        if let Some(id) = hit.get("id").and_then(|x| x.as_str()) {
            // preprint with full text
            let xml = europe_pmc_full_text(id, o)?;
            return Ok(Source::Jats {
                data: xml,
                assets: AssetLocator::None,
                meta,
                label: "Europe PMC preprint XML".into(),
            });
        }
    }
    let journal = if meta.journal.is_empty() { "the publisher".to_string() } else { meta.journal.clone() };
    Err(fail(format!(
        "“{}” isn't in PubMed Central, so there is no XML version to convert. It may still be free on {journal}'s site as a PDF. PDF conversion isn't in the app yet.",
        if meta.title.is_empty() { what } else { &meta.title }
    )))
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

pub fn epmc_meta(r: &Value) -> PaperMeta {
    let mut m = PaperMeta {
        title: s(r, "title").trim_matches(|c| c == '.' || c == ' ').to_string(),
        ..Default::default()
    };
    if let Some(a) = r.get("authorList").and_then(|x| x.get("author")).and_then(|x| x.as_array()) {
        m.authors = a.iter().map(|x| s(x, "fullName")).filter(|x| !x.is_empty()).collect();
    }
    m.date = s(r, "firstPublicationDate");
    m.doi = s(r, "doi");
    m.journal = r
        .get("journalInfo")
        .and_then(|x| x.get("journal"))
        .map(|x| s(x, "title"))
        .unwrap_or_default();
    static TAG: OnceLock<Regex> = OnceLock::new();
    m.abstract_text = TAG.get_or_init(|| Regex::new("<[^>]+>").unwrap()).replace_all(&s(r, "abstractText"), "").into_owned();
    m
}

pub fn europe_pmc_full_text(id: &str, o: &Options) -> Result<Vec<u8>> {
    o.log(&format!("Downloading full-text XML for {id} from Europe PMC"));
    let url = format!("https://www.ebi.ac.uk/europepmc/webservices/rest/{id}/fullTextXML");
    let xml = net::with_retry(2, || net::get(&url, None, 90.0))?;
    if sniff(&xml) != "jats" {
        return Err(fail(format!("Europe PMC returned no XML for {id}")));
    }
    Ok(xml)
}

pub const S3_BASE: &str = "https://pmc-oa-opendata.s3.amazonaws.com/";

/// Prefers the PMC open-data bucket (XML + figures side by side); falls back to Europe PMC's XML.
pub fn pmc_source(pmcid: &str, o: &Options) -> Result<Source> {
    o.log(&format!("Checking the PMC open-access archive for {pmcid}"));
    if let Ok((folder, files)) = s3_listing(pmcid) {
        if let Some(xml_name) = files.iter().find(|f| f.ends_with(".xml")) {
            let base = format!("{S3_BASE}{}", net::encode_path(&folder));
            let url = format!("{base}/{}", net::encode_path(xml_name));
            if let Ok(xml) = net::with_retry(2, || net::get(&url, None, 90.0)) {
                if sniff(&xml) == "jats" {
                    return Ok(Source::Jats {
                        data: xml,
                        assets: AssetLocator::S3 { base, files },
                        meta: PaperMeta::default(),
                        label: "PMC open-access XML".into(),
                    });
                }
            }
        }
    }
    let xml = europe_pmc_full_text(pmcid, o)?;
    Ok(Source::Jats { data: xml, assets: AssetLocator::None, meta: PaperMeta::default(), label: "Europe PMC XML".into() })
}

/// Lists the newest version folder for a PMCID in the PMC open-data bucket.
pub fn s3_listing(pmcid: &str) -> Result<(String, Vec<String>)> {
    let url = format!("{S3_BASE}?list-type=2&prefix={}.&max-keys=1000", net::encode_query(pmcid));
    let d = net::get(&url, None, 30.0)?;
    let body = String::from_utf8_lossy(&d);
    let mut by_version: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for c in Regex::new("<Key>([^<]+)</Key>").unwrap().captures_iter(&body) {
        let key = &c[1];
        let Some((folder, file)) = key.split_once('/') else { continue };
        let Some((_, v)) = folder.rsplit_once('.') else { continue };
        let Ok(v) = v.parse::<u64>() else { continue };
        by_version.entry(v).or_default().push(file.to_string());
    }
    let Some((v, files)) = by_version.into_iter().next_back() else {
        return Err(fail("not in PMC open-access archive"));
    };
    Ok((format!("{pmcid}.{v}"), files))
}

// MARK: - eLife

pub fn elife_source(num: &str, o: &Options) -> Result<Source> {
    let n = format!("{}{num}", "0".repeat(5usize.saturating_sub(num.len())));
    o.log(&format!("Downloading eLife XML {n}"));
    for v in [3, 2, 1] {
        let url =
            format!("https://raw.githubusercontent.com/elifesciences/elife-article-xml/master/articles/elife-{n}-v{v}.xml");
        if let Ok(d) = net::get(&url, None, 90.0) {
            if sniff(&d) == "jats" {
                return Ok(Source::Jats {
                    data: d,
                    assets: AssetLocator::Patterns(vec![
                        format!("https://iiif.elifesciences.org/lax:{n}%2F{{href}}/full/1500,/0/default.jpg"),
                        format!("https://iiif.elifesciences.org/lax/{n}%2F{{href}}/full/1500,/0/default.jpg"),
                        format!("https://cdn.elifesciences.org/articles/{n}/{{stem}}.jpg"),
                    ]),
                    meta: PaperMeta::default(),
                    label: "eLife XML".into(),
                });
            }
        }
    }
    Err(fail(format!("eLife XML not found for {n}")))
}

// MARK: - Publisher pages

/// Reads a publisher page's `citation_*` meta tags to find the DOI (or a PDF link).
pub fn from_web_page(url: &url::Url, o: &Options) -> Result<Source> {
    let host = url.host_str().unwrap_or("").to_string();
    o.log(&format!("Reading {}", if host.is_empty() { "the page" } else { &host }));
    let r = net::fetch(url.as_str(), Some("text/html,application/xhtml+xml,application/pdf;q=0.9,*/*;q=0.8"), 25.0)
        .map_err(|e| fail(format!("Could not open {}: {e}", if host.is_empty() { "the link" } else { &host })))?;
    let kind = sniff(&r.data);
    if kind == "pdf" && (200..300).contains(&r.status) {
        return Ok(Source::Pdf {
            data: r.data,
            meta: PaperMeta { url: url.to_string(), ..Default::default() },
            label: "PDF from link".into(),
        });
    }
    if kind == "jats" {
        return Ok(Source::Jats {
            data: r.data,
            assets: AssetLocator::None,
            meta: PaperMeta::default(),
            label: "XML from link".into(),
        });
    }
    if !(200..300).contains(&r.status) {
        return Err(fail(format!(
            "{} refused the request (HTTP {}). Try the paper's DOI, or open the PDF in a browser and convert the file.",
            if host.is_empty() { "The site" } else { &host },
            r.status
        )));
    }
    let html = String::from_utf8_lossy(&r.data[..r.data.len().min(1_500_000)]).into_owned();
    let tags = meta_tags(&html);
    let mut doi = ["citation_doi", "prism.doi", "dc.identifier", "bepress_citation_doi"]
        .iter()
        .find_map(|k| tags.get(*k).cloned());
    if doi.is_none() {
        if let Some(canon) = first_match(r#"<link[^>]+rel="canonical"[^>]+href="([^"]+)""#, &html, false) {
            doi = doi_in(&canon);
        }
    }
    if let Some(d) = doi.and_then(|d| doi_in(&d)) {
        o.log(&format!("Found DOI {d}"));
        let mut last = fail(format!("No open full text found for {d}"));
        for c in doi_candidates(&d) {
            let label = c.label.clone();
            match (c.load)(o) {
                Ok(s) => return Ok(s),
                Err(e) => {
                    o.log(&format!("  {label}: {e}"));
                    last = e;
                }
            }
        }
        if let Some(pdf) = tags.get("citation_pdf_url") {
            o.log(&format!("The page links a PDF: {pdf}"));
        }
        return Err(last);
    }
    Err(fail("Couldn't find a DOI on that page. Try the DOI directly, or convert the PDF."))
}

/// `<meta name=… content=…>` pairs, lower-cased names, first occurrence wins.
pub fn meta_tags(html: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let tag_re = re(r"<meta\s[^>]*>", true);
    let attr_re = Regex::new(r#"([a-zA-Z:-]+)\s*=\s*("([^"]*)"|'([^']*)')"#).unwrap();
    for m in tag_re.find_iter(html) {
        let mut a: HashMap<String, String> = HashMap::new();
        for c in attr_re.captures_iter(m.as_str()) {
            let k = c[1].to_lowercase();
            let v = c.get(3).or(c.get(4)).map(|x| x.as_str()).unwrap_or("").to_string();
            a.insert(k, v);
        }
        if let (Some(n), Some(c)) = (a.get("name").or(a.get("property")), a.get("content")) {
            out.entry(n.to_lowercase()).or_insert_with(|| c.replace("&amp;", "&"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inputs() {
        assert_eq!(arxiv_id("1706.03762").as_deref(), Some("1706.03762"));
        assert_eq!(arxiv_id("https://arxiv.org/abs/1706.03762v7").as_deref(), Some("1706.03762v7"));
        assert_eq!(arxiv_id("https://arxiv.org/pdf/1706.03762.pdf").as_deref(), Some("1706.03762"));
        assert_eq!(arxiv_id("10.48550/arXiv.1706.03762").as_deref(), Some("1706.03762"));
        assert_eq!(arxiv_id("hep-th/9901001").as_deref(), Some("hep-th/9901001"));
        assert_eq!(arxiv_id("10.1038/nature12373"), None);
        assert_eq!(doi_in("https://doi.org/10.1038/s41586-020-2012-7").as_deref(), Some("10.1038/s41586-020-2012-7"));
        assert_eq!(doi_in("https://www.biorxiv.org/content/10.1101/2024.01.02.573123v2.full.pdf").as_deref(), Some("10.1101/2024.01.02.573123"));
        assert_eq!(doi_in("https://x.org/doi/10.1000%2Fabc/full").as_deref(), Some("10.1000/abc"));
        assert_eq!(
            first_match(r"\b(PMC\d{4,})\b", "https://www.ncbi.nlm.nih.gov/pmc/articles/pmc7029158/", true).as_deref(),
            Some("pmc7029158")
        );
        let t = meta_tags(r#"<meta name="citation_doi" content="10.1/x"><meta NAME="Citation_DOI" content="no">"#);
        assert_eq!(t.get("citation_doi").map(String::as_str), Some("10.1/x"));
    }
}
