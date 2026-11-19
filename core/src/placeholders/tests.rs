use super::*;
use std::sync::Arc;

fn found(text: &str) -> Vec<(String, Vec<(Vec<String>, bool, String, Vec<String>)>)> {
    find_placeholders(text.into())
        .into_iter()
        .map(|p| (p.text, p.cites.into_iter().map(|c| (c.surnames, c.et_al, c.year, c.hints)).collect()))
        .collect()
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn placeholders_written_in_every_way() {
    let t = "As shown (Langseth et al. 2025), and (Langseth et al., 2025; Smith 2019). Also [Langseth 2025 spatial] and {Langseth 2025}.";
    let f = found(t);
    assert_eq!(f.len(), 4, "{f:?}");
    assert_eq!(f[0].0, "(Langseth et al. 2025)");
    assert_eq!(f[0].1, vec![(names(&["Langseth"]), true, "2025".into(), vec![])]);
    assert_eq!(f[1].1.len(), 2);
    assert_eq!(f[1].1[1], (names(&["Smith"]), false, "2019".into(), vec![]));
    assert_eq!(f[2].1, vec![(names(&["Langseth"]), false, "2025".into(), names(&["spatial"]))]);
    assert_eq!(f[3].0, "{Langseth 2025}");
    // offsets in UTF-16, as JavaScript and Word count
    let p = find_placeholders(t.into());
    let u: Vec<u16> = t.encode_utf16().collect();
    for x in &p {
        assert_eq!(String::from_utf16(&u[x.start as usize..x.end as usize]).unwrap(), x.text);
    }
}

#[test]
fn placeholder_names_accents_hyphens_particles() {
    let t = "ü (Gonçalo 2020) — (Castelo-Branco & Kukanja 2024a) (van der Berg, 2018) (O’Brien and Smith 2019) (Smith, Jones, and Lee 2017) (see Østergaard et al., 2021) (Smith 2019, 2020)";
    let f = found(t);
    assert_eq!(f.len(), 7, "{f:?}");
    assert_eq!(f[0].1[0].0, names(&["Gonçalo"]));
    assert_eq!(f[1].1[0].0, names(&["Castelo-Branco", "Kukanja"]));
    assert_eq!(f[1].1[0].2, "2024");
    assert_eq!(f[2].1[0].0, names(&["van der Berg"]));
    assert_eq!(f[3].1[0].0, names(&["O’Brien", "Smith"]));
    assert_eq!(f[4].1[0].0, names(&["Smith", "Jones", "Lee"]));
    assert_eq!(f[5].1[0].0, names(&["Østergaard"]));
    assert!(f[5].1[0].1);
    // two years: two papers by the same authors
    assert_eq!(f[6].1.iter().map(|c| c.2.clone()).collect::<Vec<_>>(), names(&["2019", "2020"]));
    // UTF-16 offsets past "ü" and "—"
    let p = find_placeholders(t.into());
    let u: Vec<u16> = t.encode_utf16().collect();
    assert_eq!(String::from_utf16(&u[p[1].start as usize..p[1].end as usize]).unwrap(), "(Castelo-Branco & Kukanja 2024a)");
}

#[test]
fn ordinary_brackets_are_not_placeholders() {
    for t in [
        "(see Fig. 2)", "(n = 2025)", "(p < 0.05)", "(2019–2020)", "(Table 1)", "(Fig. 3, 2019)", "(January 2020)", "(Table 2019)",
        "[1]", "[1-3, 7]", "(i.e., the controls)", "(in 2019)", "(Supplementary Table 3)", "{x}", "(COVID 2019)",
        "(Smith 2019, p < 0.05)", "(Smith 2019 found the same thing in mice and in rats)", "[Smith 2019](https://example.org)",
        "(Smith et al.)", "(Smith)", "(Smith 20190)", "(smith 2019)",
    ] {
        assert!(find_placeholders(t.into()).is_empty(), "{t} was taken for a placeholder");
    }
    // one bad part spoils the whole: "(Smith 2019; Fig. 2)"
    assert!(find_placeholders("(Smith 2019; Fig. 2)".into()).is_empty());
    // the inner one of nested brackets
    assert_eq!(found("(see also (Smith 2019))")[0].0, "(Smith 2019)");
}

#[test]
fn latex_placeholders_and_rewriting() {
    let tex = "Cells \\cite{?langseth2025} and \\citep[see][p.~4]{smith2019, langseth2025spatial?}.\n\
               % \\cite{?commented2020} (Hidden 2020)\n\
               Also (Langseth et al. 2025) and \\cite{plain2019}; 100\\% (Wu 2018). {Langseth 2025}\n";
    let p = find_tex_placeholders(tex.into());
    let texts: Vec<&str> = p.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, ["\\cite{?langseth2025}", "\\citep[see][p.~4]{smith2019, langseth2025spatial?}", "(Langseth et al. 2025)", "(Wu 2018)"]);
    assert_eq!(p[0].cites[0].surnames, names(&["Langseth"]));
    assert_eq!(p[0].cites[0].year, "2025");
    assert_eq!(p[0].cites[0].key, "langseth2025");
    assert_eq!(p[1].cites.len(), 1);
    assert_eq!(p[1].cites[0].hints, names(&["spatial"]));
    assert_eq!(p[1].command, "\\citep[see][p.~4]");
    assert_eq!(p[1].keys, names(&["smith2019", "langseth2025spatial?"]));

    let choices = vec![
        PlaceholderChoice { start: p[0].start, end: p[0].end, keys: names(&["langseth2025cellular"]) },
        PlaceholderChoice { start: p[1].start, end: p[1].end, keys: names(&["langseth2025spatial"]) },
        PlaceholderChoice { start: p[2].start, end: p[2].end, keys: names(&[""]) }, // skipped
        PlaceholderChoice { start: p[3].start, end: p[3].end, keys: names(&["wu2018b"]) },
        PlaceholderChoice { start: 1, end: 3, keys: names(&["nowhere"]) },          // not a placeholder: ignored
    ];
    let out = rewrite_tex_placeholders(tex.into(), choices);
    assert_eq!(
        out,
        "Cells \\cite{langseth2025cellular} and \\citep[see][p.~4]{smith2019,langseth2025spatial}.\n\
         % \\cite{?commented2020} (Hidden 2020)\n\
         Also (Langseth et al. 2025) and \\cite{plain2019}; 100\\% \\cite{wu2018b}. {Langseth 2025}\n"
    );
    // a part left as written after the citation
    let t = "x (Langseth et al., 2025; Smith 2019) y";
    let p = find_tex_placeholders(t.into());
    let out = rewrite_tex_placeholders(t.into(), vec![PlaceholderChoice { start: p[0].start, end: p[0].end, keys: names(&["langseth2025", ""]) }]);
    assert_eq!(out, "x \\cite{langseth2025} (Smith 2019) y");
}

// ---- suggestions ----

fn library() -> (Arc<Library>, std::path::PathBuf) {
    let d = std::env::temp_dir().join(format!("lunta-placeholders-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    let lib = Library::open(d.join("lib").to_string_lossy().into(), d.join("cache").to_string_lossy().into(), "test".into()).unwrap();
    (lib, d)
}

fn paper(title: &str, authors: &[&str], date: &str, doi: &str) -> PaperMeta {
    PaperMeta { title: title.into(), authors: names(authors), date: date.into(), journal: "Journal".into(), doi: doi.into(), ..Default::default() }
}

fn cite(text: &str) -> PlaceholderCite {
    find_placeholders(text.into()).remove(0).cites.remove(0)
}

fn no_web(_: &str) -> Result<Value> {
    panic!("asked the web")
}

fn offline(_: &str) -> Result<Value> {
    Err(CoreError::Network("offline".into()))
}

#[test]
fn suggestions_from_the_library_ranked() {
    let (lib, dir) = library();
    let spatial = lib.add_paper(paper("Spatial transcriptomics of the mouse brain", &["Christoffer Mattsson Langseth", "A B", "C D"], "2025-02", "10.1/a"), None, None, None).unwrap();
    let lesions = lib.add_paper(paper("Lesions in multiple sclerosis", &["Petra Kukanja", "Christoffer M. Langseth", "Gonçalo Castelo-Branco"], "2025", "10.1/b"), None, None, None).unwrap();
    let older = lib.add_paper(paper("Cell typing in situ", &["Langseth, Christoffer"], "2023", "10.1/c"), None, None, None).unwrap();
    let smith = lib.add_paper(paper("Unrelated", &["Jane Smith"], "2019", "10.1/d"), None, None, None).unwrap();
    let goncalo = lib.add_paper(paper("Oligodendrocytes", &["Gonçalo Castelo-Branco", "Kim Ng"], "2020", "10.1/e"), None, None, None).unwrap();

    // first author and year: one paper, exact; no need to ask the web
    let s = suggest_with(&lib, &cite("(Langseth et al. 2025)"), true, &no_web);
    assert_eq!(s.candidates[0].paper_id, spatial.id);
    assert!(s.exact && s.candidates[0].exact);
    // second author too, but lower, and not exact; the 2023 paper lower still
    let ids: Vec<&str> = s.candidates.iter().map(|c| c.paper_id.as_str()).collect();
    assert_eq!(ids, [spatial.id.as_str(), lesions.id.as_str(), older.id.as_str()]);
    assert!(!s.candidates[1].exact);
    assert!(s.candidates[0].confidence > s.candidates[1].confidence && s.candidates[1].confidence > s.candidates[2].confidence);

    // hint words decide between two papers of the same author and year
    lib.add_paper(paper("Another spatial atlas", &["Christoffer Mattsson Langseth"], "2025", "10.1/f"), None, None, None).unwrap();
    let s = suggest_with(&lib, &cite("(Langseth 2025)"), false, &no_web);
    assert!(!s.exact, "two papers fit: not exact");
    let s = suggest_with(&lib, &cite("[Langseth 2025 mouse brain]"), false, &no_web);
    assert_eq!(s.candidates[0].paper_id, spatial.id);
    assert!(s.exact);

    // a hyphenated family name, alone or with a second author
    let s = suggest_with(&lib, &cite("(Castelo-Branco 2020)"), false, &no_web);
    assert_eq!(s.candidates[0].paper_id, goncalo.id);
    let s = suggest_with(&lib, &cite("(Castelo-Branco and Ng 2020)"), false, &no_web);
    assert!(s.exact && s.candidates[0].paper_id == goncalo.id);
    // a double family name, written whole or as its last part
    let s = suggest_with(&lib, &cite("(Mattsson Langseth et al. 2025)"), false, &no_web);
    assert_eq!(s.candidates[0].paper_id, spatial.id);

    // a LaTeX key: the paper's own key is certain
    let key = lib.cite_keys(vec![older.id.clone()]).remove(0);
    let tex = format!("\\cite{{?{key}}}");
    let c = find_tex_placeholders(tex).remove(0).cites.remove(0);
    let s = suggest_with(&lib, &c, false, &no_web);
    assert_eq!((s.candidates[0].paper_id.as_str(), s.exact), (older.id.as_str(), true));

    // nothing in the library, Crossref offline: said so, nothing else
    let s = suggest_with(&lib, &cite("(Nobody 2019)"), true, &offline);
    assert!(s.candidates.is_empty() && s.note.contains("Crossref"), "{s:?}");
    let s = suggest_with(&lib, &cite("(Smith 2019)"), false, &no_web);
    assert_eq!(s.candidates[0].paper_id, smith.id);
    let _ = std::fs::remove_dir_all(&dir);
}

fn fixture(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/placeholders/fixtures").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn suggestions_from_crossref_and_openalex() {
    let (lib, dir) = library();
    // recorded answers (Crossref's /works with query.author, OpenAlex's /works search)
    let asked = std::cell::RefCell::new(vec![]);
    let web = |u: &str| -> Result<Value> {
        asked.borrow_mut().push(u.to_string());
        Ok(if u.contains("api.crossref.org") && u.contains("query.bibliographic") {
            fixture("crossref-theodoris-network-biology.json")
        } else if u.contains("api.crossref.org") {
            fixture("crossref-theodoris-2022-2024.json")
        } else {
            fixture("openalex-theodoris-network-biology.json")
        })
    };
    let s = suggest_with(&lib, &cite("[Theodoris et al. 2023 network biology]"), true, &web);
    assert!(asked.borrow()[0].contains("query.author=Theodoris") && asked.borrow()[0].contains("from-pub-date:2022,until-pub-date:2024"), "{:?}", asked.borrow());
    assert_eq!(s.candidates[0].meta.doi, "10.1038/s41586-023-06139-9", "{s:#?}");
    assert_eq!(s.candidates[0].source, "crossref");
    assert!(s.candidates[0].paper_id.is_empty() && !s.exact);
    // the 2024 single-author perspective fits less; "Hao et al." (not by Theodoris) not at all
    assert!(s.candidates.iter().all(|c| c.meta.authors.iter().any(|a| a.contains("Theodoris"))));
    assert!(s.candidates[0].confidence > s.candidates[1].confidence);

    // without hint words: Theodoris's papers of the years around; as first author before the
    // one of the year itself where Theodoris is second
    let s = suggest_with(&lib, &cite("(Theodoris 2023)"), true, &web);
    assert!(s.candidates.iter().all(|c| c.meta.authors.iter().any(|a| a.contains("Theodoris"))), "{s:#?}");
    assert!(s.candidates[0].meta.authors[0].contains("Theodoris"));
    let second = s.candidates.iter().position(|c| c.meta.doi == "10.1038/s43588-023-00570-8").unwrap();
    assert!(second > 0);

    // a paper the library has comes as the library's, before the others
    let p = lib.add_paper(paper("Perspectives on benchmarking foundation models for network biology", &["Christina V. Theodoris"], "2024", "10.1002/qub2.68"), None, None, None).unwrap();
    let s = suggest_with(&lib, &cite("[Theodoris et al. 2023 network biology]"), true, &web);
    assert_eq!(s.candidates[0].paper_id, p.id);
    assert_eq!(s.candidates[1].meta.doi, "10.1038/s41586-023-06139-9");

    // Crossref with nothing: OpenAlex
    let empty = |u: &str| -> Result<Value> {
        if u.contains("api.crossref.org") { Ok(serde_json::json!({"message": {"items": []}})) } else { Ok(fixture("openalex-theodoris-network-biology.json")) }
    };
    let s = suggest_with(&lib, &cite("[Theodoris 2023 transfer learning]"), true, &empty);
    let top = s.candidates.iter().find(|c| c.paper_id.is_empty()).expect(&format!("{s:#?}"));
    assert_eq!((top.source.as_str(), top.meta.doi.as_str()), ("openalex", "10.1038/s41586-023-06139-9"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn search_for_a_placeholder() {
    let (lib, dir) = library();
    let p = lib.add_paper(paper("Spatial transcriptomics of the mouse brain", &["Ada Lee"], "2025", "10.1234/abc"), None, None, None).unwrap();
    let s = search_with(&lib, "mouse brain", false, &no_web);
    assert_eq!(s.candidates[0].paper_id, p.id);
    let s = search_with(&lib, "Lee 2025", false, &no_web);
    assert!(s.exact && s.candidates[0].paper_id == p.id);
    let s = search_with(&lib, "https://doi.org/10.1234/ABC", false, &no_web);
    assert_eq!(s.candidates[0].paper_id, p.id);
    let _ = std::fs::remove_dir_all(&dir);
}
