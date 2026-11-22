//! Summarize Highlights: reading the model's answer, the checks, long sets in parts. The model's
//! answers are written here (what small models do: cite, forget to, cite what wasn't given,
//! invent, turn a statement around).

use super::*;
use std::sync::Arc;

fn src(n: u32, paper: &str, label: &str, quote: &str, note: &str) -> SummarySource {
    SummarySource {
        number: n,
        annotation_id: format!("a{n}"),
        paper_id: paper.into(),
        quote: quote.into(),
        note: note.into(),
        section: String::new(),
        label: label.into(),
        cite_key: String::new(),
        reference: format!("{label}. A paper."),
        link: highlight_link(paper, &format!("a{n}")),
    }
}

/// Five highlights in two papers, numbered 1–5.
fn sources() -> Vec<SummarySource> {
    vec![
        src(1, "p1", "Kukanja et al. 2024", "Chronic active lesions have a rim of iron-laden microglia.", ""),
        src(2, "p1", "Kukanja et al. 2024", "Remyelination is more common in early lesions.", ""),
        src(3, "p1", "Kukanja et al. 2024", "Oligodendrocytes near the rim show stress responses in 40% of lesions.", "compare with our MRI data"),
        src(4, "p2", "Ke and Nilsson 2013", "Microglia states differ between white and grey matter.", ""),
        src(5, "p2", "Ke and Nilsson 2013", "Probe design limits which transcripts are seen.", ""),
    ]
}

fn ids(s: &[SummarySource]) -> Vec<String> {
    s.iter().map(|x| x.annotation_id.clone()).collect()
}

fn one_part(sources: Vec<SummarySource>, style: SummaryStyle) -> SummaryPlan {
    let p = ModelPrompt { instructions: String::new(), prompt: String::new(), max_tokens: 400, ids: ids(&sources) };
    SummaryPlan { title: "Summary of the highlights".into(), topic: String::new(), length: SummaryLength::Short, style, papers: 2, sources, parts: vec![p], left_out: 0, note: String::new() }
}

#[test]
fn citations_are_read() {
    assert_eq!(citations("Microglia sit at lesion rims [1, 4]."), ("Microglia sit at lesion rims.".to_string(), vec![1, 4]));
    assert_eq!(citations("Rims [3–5] and more [2; 7]").1, vec![3, 4, 5, 2, 7]);
    assert_eq!(citations("Lesions [Highlight 2] [5]").1, vec![2, 5]);
    assert_eq!(citations("Lesions [1, 2 and 3]").1, vec![1, 2, 3]);
    assert_eq!(citations("Lesions [2][5].").0, "Lesions.");
    // not a citation: a word in brackets
    assert_eq!(citations("Lesions [sic] matter.").1, Vec::<u32>::new());
}

#[test]
fn sentences_are_split_where_they_end() {
    let s = sentences(
        "Here is a summary:\n\n**Summary:** Kukanja et al. 2024 describe rims (Fig. 2) of 1.5 mm [1]. Remyelination is early [2].\n\
         - Microglia differ, e.g. by matter [4].\n2. Probes limit what is seen. [5]\n# Heading\n",
    );
    assert_eq!(
        s,
        vec![
            "Kukanja et al. 2024 describe rims (Fig. 2) of 1.5 mm [1].",
            "Remyelination is early [2].",
            "Microglia differ, e.g. by matter [4].",
            "Probes limit what is seen. [5]",
        ]
    );
    // a citation before the full stop, after it, or in the middle
    assert_eq!(sentences("A is B [1]. C is done. [2] E [3] is F."), vec!["A is B [1].", "C is done. [2]", "E [3] is F."]);
    assert_eq!(sentences("<think>hmm. [9]</think>Only this [1]."), vec!["Only this [1]."]);
}

#[test]
fn the_check_drops_what_isnt_cited_or_supported() {
    let s = sources();
    let answer = "Chronic active lesions have rims of iron-laden microglia [1].\n\
                  Microglia cause multiple sclerosis.\n\
                  Remyelination is more common in early lesions [9].\n\
                  Microglia states differ between white and grey matter [4].\n\
                  The cortex is made of cheese and governed by quantum effects [5].\n\
                  Remyelination is not more common in early lesions [2].\n\
                  Stress responses appear in 75% of lesions near the rim [3].\n\
                  Oligodendrocytes near the rim show stress responses in 40% of lesions [3].\n\
                  Microglia states differ between white and grey matter [4].";
    let (kept, drops) = validate(answer, &ids(&s), &s, None, 10);
    let texts: Vec<&str> = kept.iter().map(|k| k.text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "Chronic active lesions have rims of iron-laden microglia.",
            "Microglia states differ between white and grey matter.",
            "Oligodendrocytes near the rim show stress responses in 40% of lesions.",
        ],
        "the repeat is kept once"
    );
    assert_eq!(drops, Drops { uncited: 1, unknown: 1, unsupported: 3 }, "cheese, the negation, the invented 75 %");
    assert_eq!(kept[1].citations, vec![4]);
    assert!(drops.says().starts_with("Left out 5 of the model's sentences: 1 without a citation, 1 citing a highlight that doesn't exist, 3 saying more"), "{}", drops.says());
}

#[test]
fn support_allows_other_word_forms_and_the_researchers_note() {
    let s = sources();
    let cited = |n: &[usize]| n.iter().map(|i| &s[i - 1]).collect::<Vec<_>>();
    assert!(supported("Remyelination happens more commonly in early lesions", &cited(&[2])));
    assert!(supported("The researcher wants to compare the rim's stress responses with MRI data", &cited(&[3])));
    assert!(supported("Kukanja et al. report that microglia states differ by matter", &cited(&[1, 4])));
    // a word that only shares a short beginning isn't the same word
    assert!(!supported("Microscopy shows probes", &cited(&[4])));
    // citing the wrong highlight
    assert!(!supported("Probe design limits which transcripts are seen", &cited(&[1])));
    // a negation the highlight doesn't have
    assert!(!supported("Probe design does not limit which transcripts are seen", &cited(&[5])));
    assert!(!supported("Probe design doesn't limit which transcripts are seen", &cited(&[5])));
}

#[test]
fn the_summary_is_renumbered_and_its_sources_listed() {
    let plan = one_part(sources(), SummaryStyle::Prose);
    let answer = "Microglia states differ between white and grey matter [4]. Chronic active lesions have a rim of iron-laden microglia, \
                  and remyelination is more common in early lesions [2, 1]. Probe design limits which transcripts are seen [5]. Invented sentence.";
    let o = from_answers(&plan, &[answer.into()], None);
    assert!(o.ok);
    assert_eq!(o.dropped, 1);
    // first cited first: 4 → 1, 2 → 2, 1 → 3, 5 → 4
    let nums: Vec<Vec<u32>> = o.sentences.iter().map(|s| s.citations.clone()).collect();
    assert_eq!(nums, vec![vec![1], vec![2, 3], vec![4]]);
    let quotes: Vec<(u32, &str)> = o.sources.iter().map(|s| (s.number, &s.quote[..10])).collect();
    assert_eq!(quotes, vec![(1, "Microglia "), (2, "Remyelinat"), (3, "Chronic ac"), (4, "Probe desi")]);
    assert_eq!(o.sources[0].annotation_id, "a4");
    let md = &o.markdown;
    assert!(md.starts_with("# Summary of the highlights\n\n*Written by the on-device model from your highlights. Check it against the quotes below.*\n\n"), "{md}");
    assert!(md.contains("Microglia states differ between white and grey matter [1]. Chronic active"), "{md}");
    assert!(md.contains("[1] “Microglia states differ between white and grey matter.” — [Ke and Nilsson 2013](lunta://open?id=p2&highlight=a4)"), "{md}");
    assert!(md.contains("## Sources\n\n- **Ke and Nilsson 2013**: Ke and Nilsson 2013. A paper. [Open in Lunta](lunta://open?id=p2)"), "{md}");
    assert!(o.note.starts_with("Left out 1 of the model's sentences: 1 without a citation."), "{}", o.note);
    assert!(o.html.contains("[<a href=\"lunta://open?id=p1&amp;highlight=a2\">2</a>, <a href=\"lunta://open?id=p1&amp;highlight=a1\">3</a>]."), "{}", o.html);
    // bullets
    let o = from_answers(&one_part(sources(), SummaryStyle::Bullets), &["- Probe design limits which transcripts are seen [5].\n- Microglia states differ by matter [4].".into()], None);
    assert!(o.bullets && o.markdown.contains("\n- Probe design limits which transcripts are seen [1].\n- Microglia states differ by matter [2].\n"), "{}", o.markdown);
}

#[test]
fn nothing_holds_up_then_no_summary() {
    let o = from_answers(&one_part(sources(), SummaryStyle::Prose), &["Microglia are the cause of everything. The cortex is made of cheese [2].".into()], None);
    assert!(!o.ok);
    assert!(o.sentences.is_empty() && o.markdown.is_empty());
    assert!(o.note.contains("didn't hold up") && o.note.contains("outline is shown instead"), "{}", o.note);
    assert_eq!(o.dropped, 2);
    let o = from_answers(&one_part(sources(), SummaryStyle::Prose), &[String::new()], None);
    assert!(!o.ok);
}

#[test]
fn long_sets_in_parts_keep_the_original_numbers() {
    // parts of at most 2: paper 1's three highlights don't fit one, paper 2's two make one
    let items: Vec<OutlineItem> = sources()
        .iter()
        .map(|s| OutlineItem { annotation_id: s.annotation_id.clone(), paper_id: s.paper_id.clone(), quote: s.quote.clone(), note: s.note.clone(), section: String::new(), order: s.number as u64 })
        .collect();
    let paper = |id: &str, a: &str, y: &str| Paper { id: id.into(), meta: PaperMeta { title: "T".into(), authors: vec![a.into()], date: y.into(), ..Default::default() }, added: 0, modified: 0, has_pdf: false, has_epub: false, has_audio: false, read: false, starred: false, must_read: false, collections: vec![], tags: vec![], annotation_count: 0, cite_key: String::new() };
    let papers = vec![paper("p1", "Petra Kukanja", "2024"), paper("p2", "Rongqin Ke", "2013")];
    let plan = plan(&items, &papers, "", SummaryLength::Short, SummaryStyle::Bullets, 2, 6, String::new());
    let part_ids: Vec<Vec<String>> = plan.parts.iter().map(|p| p.ids.clone()).collect();
    assert_eq!(part_ids, vec![vec!["a1", "a2"], vec!["a3"], vec!["a4", "a5"]]);
    // each part numbers its own highlights from 1
    assert!(plan.parts[2].prompt.contains("[1] (Ke 2013) \"Microglia states differ"), "{}", plan.parts[2].prompt);
    assert!(plan.parts[1].prompt.contains("Researcher's note: compare with our MRI data"));
    assert!(plan.parts[0].instructions.contains("2 to 3 short sentences as one paragraph"), "parts are prose: {}", plan.parts[0].instructions);
    let answers = vec![
        "Chronic active lesions have a rim of iron-laden microglia [1]. Remyelination is more common in early lesions [2].".to_string(),
        "Oligodendrocytes near the rim show stress responses [1]. They also glow in the dark [1].".to_string(),
        "Microglia states differ between white and grey matter [1]. Probe design limits which transcripts are seen [2].".to_string(),
    ];
    let c = combine_prompt(&plan, &answers).expect("three parts to combine");
    // the parts' sentences with the plan's numbers
    assert!(c.prompt.contains("Part 1 (Kukanja 2024):\nChronic active lesions have a rim of iron-laden microglia [1].\nRemyelination is more common in early lesions [2]."), "{}", c.prompt);
    assert!(c.prompt.contains("Part 2 (Kukanja 2024):\nOligodendrocytes near the rim show stress responses [3]."), "{}", c.prompt);
    assert!(c.prompt.contains("Part 3 (Ke 2013):\nMicroglia states differ between white and grey matter [4].\nProbe design limits which transcripts are seen [5]."), "{}", c.prompt);
    assert!(!c.prompt.contains("glow"));
    assert!(c.instructions.contains("3 to 5 short sentences as bullet points"));
    // combined: a number the parts didn't cite is refused; renumbered as first cited
    let combined = "- Microglia states differ between white and grey matter, and chronic active lesions have a rim of iron-laden microglia [4, 1].\n\
                    - Probe design limits which transcripts are seen [5].\n\
                    - Oligodendrocytes near the rim show stress responses in 40% of lesions [3].\n\
                    - Remyelination is more common in early lesions [6].";
    let o = from_answers(&plan, &answers, Some(combined));
    assert!(o.ok);
    let nums: Vec<Vec<u32>> = o.sentences.iter().map(|s| s.citations.clone()).collect();
    assert_eq!(nums, vec![vec![1, 2], vec![3], vec![4]]);
    let ann: Vec<&str> = o.sources.iter().map(|s| s.annotation_id.as_str()).collect();
    assert_eq!(ann, vec!["a4", "a1", "a5", "a3"]);
    assert_eq!(o.dropped, 2, "the parts' “glow” and the combined [6]");
    // combining gave nothing usable: the parts' sentences
    let o = from_answers(&plan, &answers, Some("I can't do that."));
    assert!(o.ok);
    assert_eq!(o.sentences.len(), 5);
    assert!(o.note.contains("couldn't combine the parts"), "{}", o.note);
    assert_eq!(o.sources.iter().map(|s| s.annotation_id.as_str()).collect::<Vec<_>>(), vec!["a1", "a2", "a3", "a4", "a5"]);
    // one part with sentences: nothing to combine
    assert!(combine_prompt(&plan, &[answers[0].clone(), String::new(), "Nothing.".into()]).is_none());
    // more than fit
    let small = super::plan(&items, &papers, "", SummaryLength::Short, SummaryStyle::Prose, 4, 1, String::new());
    assert_eq!((small.sources.len(), small.left_out, small.parts.len()), (4, 1, 1));
    assert!(small.note.contains("The first 4 were summarized; 1 highlight or note didn't fit"), "{}", small.note);
    assert!(small.parts[0].instructions.contains("3 to 5 short sentences as one paragraph"));
}

#[test]
fn a_plan_from_the_library() {
    let d = tempfile::tempdir().unwrap();
    let l: Arc<Library> = Library::open(d.path().join("lib").to_string_lossy().into(), d.path().join("cache").to_string_lossy().into(), "test".into()).unwrap();
    let a = l
        .add_paper(
            PaperMeta { title: "Lesions".into(), authors: vec!["Petra Kukanja".into(), "B".into(), "C".into()], date: "2024".into(), ..Default::default() },
            None,
            None,
            Some("Chronic active lesions have a rim of iron-laden microglia. Remyelination is more common in early lesions.".into()),
        )
        .unwrap()
        .id;
    l.add_highlight(a.clone(), "Chronic active lesions have a rim of iron-laden microglia.".into(), None, "yellow".into(), String::new()).unwrap();
    l.add_highlight(a.clone(), "Remyelination is more common in early lesions.".into(), None, "yellow".into(), String::new()).unwrap();
    let p = l.summary_plan(vec![a.clone()], String::new(), SummaryLength::Longer, SummaryStyle::Prose, 25, 6);
    assert_eq!((p.sources.len(), p.parts.len(), p.papers), (2, 1, 1));
    assert!(p.parts[0].instructions.contains("6 to 10 short sentences"));
    assert!(p.parts[0].prompt.starts_with("Highlights:\n[1] (Kukanja et al. 2024) \""), "{}", p.parts[0].prompt);
    assert!(p.sources[0].link.starts_with(&format!("lunta://open?id={a}&highlight=")));
    // a topic: only what mentions it, and the prompt says so
    let p = l.summary_plan(vec![a.clone()], "microglia".into(), SummaryLength::Short, SummaryStyle::Prose, 25, 6);
    assert_eq!(p.sources.len(), 1);
    assert!(p.parts[0].prompt.starts_with("The summary is about: microglia."));
    assert_eq!(p.title, "Summary: microglia");
    assert!(p.note.contains("left out"), "{}", p.note);
    let answer = format!("Chronic active lesions have a rim of iron-laden microglia [1].");
    let o = summary_from_answers(p, vec![answer], None);
    assert!(o.ok && o.sources.len() == 1);
}
