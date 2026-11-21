use super::*;

const JEKYLL: &str = include_str!("../testdata/web/jekyll-blog.html");
const SUBSTACK: &str = include_str!("../testdata/web/substack-post.html");
const NEWS: &str = include_str!("../testdata/web/news-article.html");
const DOCS: &str = include_str!("../testdata/web/docs-page.html");
const SCHOLARLY: &str = include_str!("../testdata/web/scholarly-page.html");
pub(crate) const KARPATHY_URL: &str = "https://karpathy.github.io/2019/04/25/recipe/";

#[test]
fn jekyll_blog_post() {
    let m = page_meta(JEKYLL, KARPATHY_URL, "2026-11-21");
    assert_eq!(m.title, "A Recipe for Training Neural Networks");
    // the page names no author: the personal blog's name does (its family name is the host)
    assert_eq!(m.authors, vec!["Andrej Karpathy"]);
    // the date from the address (the page has none in its markup)
    assert_eq!(m.date, "2019-04-25");
    assert_eq!(m.journal, "Andrej Karpathy blog");
    assert_eq!(m.work_type, "post-weblog");
    assert_eq!(m.url, KARPATHY_URL);
    assert_eq!(m.accessed, "2026-11-21");
    // the site's motto isn't the post's summary
    assert_eq!(m.abstract_text, "");
    assert!(m.doi.is_empty());
}

#[test]
fn substack_post_from_json_ld() {
    let m = page_meta(SUBSTACK, "https://gradientnotes.substack.com/p/why-small-models-win?utm_source=x&r=1#footnote", "");
    assert_eq!(m.title, "Why Small Models Win");
    // JSON-LD before Open Graph and <meta name=author>
    assert_eq!(m.authors, vec!["Maria Lindqvist", "Jonas Berg"]);
    assert_eq!(m.date, "2024-03-07");
    assert_eq!(m.journal, "The Gradient Notebook");
    // a NewsArticle on Substack is a blog post
    assert_eq!(m.work_type, "post-weblog");
    assert_eq!(m.url, "https://gradientnotes.substack.com/p/why-small-models-win?r=1");
    assert_eq!(m.accessed, today());
    assert!(m.abstract_text.starts_with("A short essay"));
}

#[test]
fn news_article_from_open_graph() {
    let m = page_meta(NEWS, "https://www.example-times.com/science/2023/city-library", "2026-11-21");
    assert_eq!(m.title, "City Opens New Science Library");
    // article:author is a URL (Facebook's page), not a name: <meta name=author> then
    assert_eq!(m.authors, vec!["Priya Raman"]);
    assert_eq!(m.date, "2023-09-14");
    assert_eq!(m.journal, "Example Times");
    assert_eq!(m.work_type, "webpage");
}

#[test]
fn page_without_author() {
    let m = page_meta(DOCS, "https://tool.example.org/docs/config", "2026-11-21");
    // " · Example Tool Docs" is the site
    assert_eq!(m.title, "Configuration");
    assert!(m.authors.is_empty(), "{:?}", m.authors);
    assert_eq!(m.date, "");
    assert_eq!(m.journal, "Example Tool Docs");
    assert_eq!(m.work_type, "webpage");
}

#[test]
fn a_page_that_declares_a_doi_is_a_paper() {
    assert_eq!(scholarly_id(SCHOLARLY).as_deref(), Some("10.1109/CVPR.2016.90"));
    assert_eq!(scholarly_id(JEKYLL), None);
    assert_eq!(read(SCHOLARLY, "https://example.org/a", "").scholarly_id, "10.1109/CVPR.2016.90");
}

#[test]
fn web_links() {
    assert!(is_web_link(KARPATHY_URL));
    assert!(is_web_link("https://example.com/blog/post"));
    assert!(!is_web_link("https://doi.org/10.1038/nature12373"));
    assert!(!is_web_link("https://arxiv.org/abs/1706.03762"));
    assert!(!is_web_link("https://www.ncbi.nlm.nih.gov/pmc/articles/PMC7029158/"));
    assert!(!is_web_link("10.1038/nature12373"));
    assert!(!is_web_link("not a link"));
    assert!(!is_web_link("https://localhost/x"));
}

#[test]
fn blogs_by_address() {
    assert!(blog_address(KARPATHY_URL));
    assert!(blog_address("https://medium.com/@someone/a-post-1234"));
    assert!(blog_address("https://someone.substack.com/p/post"));
    assert!(blog_address("https://example.wordpress.com/2020/01/02/x/"));
    assert!(blog_address("https://example.com/blog/2020/x"));
    assert!(blog_address("https://blog.example.com/x"));
    assert!(!blog_address("https://pytorch.github.io/docs/stable/"));
    assert!(!blog_address("https://www.example-times.com/science/city-library"));
}

#[test]
fn dates() {
    for (s, d) in [
        ("2019-04-25T10:00:00+00:00", "2019-04-25"),
        ("2019/04/25", "2019-04-25"),
        ("2019-04", "2019-04"),
        ("2019", "2019"),
        ("Apr 25, 2019", "2019-04-25"),
        ("April 25th, 2019", "2019-04-25"),
        ("Thursday, April 25, 2019", "2019-04-25"),
        ("25 April 2019", "2019-04-25"),
        ("Sept. 3, 2020", "2020-09-03"),
        ("March 2021", "2021-03"),
        ("04/25/2019", "2019"),
        ("yesterday", ""),
        ("", ""),
    ] {
        assert_eq!(iso_date(s), d, "{s}");
    }
    assert_eq!(date_in_url(KARPATHY_URL), "2019-04-25");
    assert_eq!(date_in_url("https://example.com/2020/07/my-post"), "2020-07");
    assert_eq!(date_in_url("https://example.com/post/2020"), "");
    assert_eq!(today().len(), 10);
}

#[test]
fn site_suffixes_off_titles() {
    assert_eq!(strip_site("Why Small Models Win | The Gradient Notebook", "The Gradient Notebook", "x.com"), "Why Small Models Win");
    assert_eq!(strip_site("The Gradient Notebook | Why Small Models Win", "The Gradient Notebook", "x.com"), "Why Small Models Win");
    assert_eq!(strip_site("Rust - the book", "", "rust-lang.org"), "Rust - the book");
}

fn karpathy() -> crate::model::Paper {
    let mut p = crate::cited_paper("k", page_meta(JEKYLL, KARPATHY_URL, "2026-11-21"));
    p.cite_key = "Karpathy2019recipe".into();
    p
}

#[test]
fn citing_a_blog_post() {
    use crate::model::CitationStyle::*;
    let p = karpathy();
    let c = |s| crate::export::citation(&p, s);
    assert_eq!(c(Apa), "Karpathy, A. (2019, April 25). A Recipe for Training Neural Networks. Andrej Karpathy blog. https://karpathy.github.io/2019/04/25/recipe/");
    assert_eq!(c(Mla), "Karpathy, Andrej. “A Recipe for Training Neural Networks.” *Andrej Karpathy blog*, 25 Apr. 2019, karpathy.github.io/2019/04/25/recipe/. Accessed 21 Nov. 2026.");
    assert_eq!(c(Chicago), "Karpathy, Andrej. 2019. “A Recipe for Training Neural Networks.” *Andrej Karpathy blog* (blog), April 25, 2019. https://karpathy.github.io/2019/04/25/recipe/.");
    assert_eq!(c(Harvard), "Karpathy, A. (2019) ‘A Recipe for Training Neural Networks’, *Andrej Karpathy blog*, 25 April. Available at: https://karpathy.github.io/2019/04/25/recipe/ (Accessed: 21 November 2026).");
    assert_eq!(c(Vancouver), "Karpathy A. A Recipe for Training Neural Networks [Internet]. Andrej Karpathy blog; 2019 Apr 25 [cited 2026 Nov 21]. Available from: https://karpathy.github.io/2019/04/25/recipe/");
    assert_eq!(c(Ieee), "A. Karpathy, “A Recipe for Training Neural Networks,” Andrej Karpathy blog, Apr. 25, 2019. Accessed: Nov. 21, 2026. [Online]. Available: https://karpathy.github.io/2019/04/25/recipe/");
    assert_eq!(c(Ama), "Karpathy A. A Recipe for Training Neural Networks. Andrej Karpathy blog. Published April 25, 2019. Accessed November 21, 2026. https://karpathy.github.io/2019/04/25/recipe/");
    assert_eq!(c(Acs), "Karpathy, A. A Recipe for Training Neural Networks. Andrej Karpathy blog. https://karpathy.github.io/2019/04/25/recipe/ (accessed 2026-11-21).");
    for s in crate::model::CitationStyle::ALL {
        assert!(c(s).contains("karpathy.github.io/2019/04/25/recipe/"), "{s:?}: {}", c(s));
    }
}

#[test]
fn citing_a_web_page_without_author_or_date() {
    let p = crate::cited_paper("d", page_meta(DOCS, "https://tool.example.org/docs/config", "2026-11-21"));
    // APA: the title (in italics: a web page) in the author's place; no date: when it was read
    assert_eq!(
        crate::export::citation(&p, crate::model::CitationStyle::Apa),
        "*Configuration*. (n.d.). Example Tool Docs. Retrieved November 21, 2026, from https://tool.example.org/docs/config"
    );
}

#[test]
fn bibtex_for_a_blog_post() {
    let b = crate::export::bibtex(&[karpathy()]);
    assert_eq!(
        b,
        "@misc{Karpathy2019recipe,\n  title = {A Recipe for Training Neural Networks},\n  author = {Karpathy, Andrej},\n  howpublished = {Andrej Karpathy blog, \\url{https://karpathy.github.io/2019/04/25/recipe/}},\n  year = {2019},\n  url = {https://karpathy.github.io/2019/04/25/recipe/},\n  date = {2019-04-25},\n  urldate = {2026-11-21},\n  note = {Accessed 2026-11-21},\n}\n\n"
    );
    // and read back: the same web page
    let e = crate::import::parse_any(&b);
    assert_eq!(e.len(), 1);
    let m = &e[0].meta;
    assert_eq!(m.work_type, "post-weblog");
    assert_eq!(m.url, KARPATHY_URL);
    assert_eq!(m.journal, "Andrej Karpathy blog");
    assert_eq!(m.accessed, "2026-11-21");
    assert_eq!(m.date, "2019-04-25");
    assert_eq!(e[0].note, "");
}

#[test]
fn ris_for_a_blog_post() {
    let r = crate::export::ris(&[karpathy()]);
    assert!(r.starts_with("TY  - BLOG\r\n"), "{r}");
    assert!(r.contains("T2  - Andrej Karpathy blog\r\n"));
    assert!(r.contains("UR  - https://karpathy.github.io/2019/04/25/recipe/\r\n"));
    assert!(r.contains("Y2  - 2026/11/21\r\n"));
    let e = crate::import::parse_any(&r);
    assert_eq!(e[0].meta.work_type, "post-weblog");
    assert_eq!(e[0].meta.accessed, "2026-11-21");
}

#[test]
fn csl_json_for_a_blog_post() {
    let p = karpathy();
    let m = crate::worktype::for_export(&p.meta);
    assert_eq!(crate::csl::csl_type(&m), "post-weblog");
    let page = crate::cited_paper("n", page_meta(NEWS, "https://www.example-times.com/science/2023/city-library", "2026-11-21"));
    assert_eq!(crate::csl::csl_type(&page.meta), "webpage");
}

/// Adding a page by its address without a scholarly identifier: as a web page, with a reading
/// view made from the page (here the HTML is given, as the browser extension sends it: no
/// network; the page has no images to fetch).
#[cfg(feature = "convert")]
#[test]
fn adding_a_web_page_without_identifier() {
    let d = tempfile::tempdir().unwrap();
    let lib = crate::Library::open(d.path().join("lib").to_string_lossy().into(), d.path().join("cache").to_string_lossy().into(), "test".into()).unwrap();
    let url = "https://tool.example.org/docs/config";
    assert!(is_web_link(url));
    let o = lib.add_web_page(url.into(), Some(DOCS.into()), None).unwrap();
    assert!(o.note.is_none(), "{:?}", o.note);
    let p = o.paper;
    assert_eq!(p.meta.title, "Configuration");
    assert_eq!(p.meta.work_type, "webpage");
    assert_eq!(p.meta.url, url);
    assert_eq!(p.meta.accessed, today());
    assert!(p.has_epub);
    let text = std::fs::read_to_string(lib.file_path(p.id.clone(), crate::model::FileKind::Fulltext).unwrap()).unwrap();
    assert!(text.contains("tool.toml"), "{text}");
    assert!(!text.contains("FAQ"));
    // the same page again (another form of its address): the same item
    let again = lib.add_web_page("http://www.tool.example.org/docs/config/?utm_source=feed".into(), Some(DOCS.into()), None).unwrap();
    assert_eq!(again.paper.id, p.id);
    assert_eq!(lib.find_by_url("tool.example.org/docs/config".into()).map(|x| x.id), Some(p.id.clone()));
    // a page with too little text: its details, and why there is no reading view
    let o = lib.add_web_page("https://app.example.com/".into(), Some("<html><head><title>App</title></head><body><p>Loading…</p></body></html>".into()), None).unwrap();
    assert!(!o.paper.has_epub);
    assert!(o.note.unwrap().contains("too little text"));
}
