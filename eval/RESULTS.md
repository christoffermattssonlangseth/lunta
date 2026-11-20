# Lunta PDF → reading view: evaluation results

Run on 2026-11-17 (macOS, Apple M1 Pro), converters of Lunta 1.0.4 (commit 86e764b); extended on
2026-11-18 with outside reference points (pdftotext, GROBID) on the same 200 PDFs and with reading
views made from the journal's article page (see the two sections after the headline numbers).
**Sixth run on 2026-11-20, both converters, commits 02a1e7f (Swift) and e1e82f0 (Rust)**: glyphs of
symbol fonts that name them like letters (Elsevier's and Springer's "Pi" fonts: Cell's Highlights
bullets as "d", "©" as "ª") read as what they show, lines starting with a bullet become list items,
and a page with no two lines side by side takes its gutter where the fewest blocks cross; see "What
the symbol-font fix changed" below. All Swift and Rust numbers in this file are from that run (the
batch speed line of `results/summary_tables.md` is not comparable: the Rust batch shared the machine
with another job); the speed numbers are still those of the 1.0.4 timing pass. The fifth-run version
of this file is kept as [RESULTS-2026-11-19b.md](RESULTS-2026-11-19b.md), its result files, figures,
examples, binaries and Swift and Rust outputs in `data/prev-2026-11-20/`.
**Fifth run on 2026-11-19, both converters, commit 7dd5b88**: the Rust converter leaves out text a
page does not show (b25a922) and puts accents drawn away from the text back on their letter
(7dd5b88); the Swift converter restores ligatures PDFKit leaves out, folds accents drawn before their
letter, and leaves out lines of glyphs drawn twice (58eaa6d); see "What the hidden-text (Rust),
ligature and accent fixes changed" below. The fourth-run version of this file is
kept as [RESULTS-2026-11-19.md](RESULTS-2026-11-19.md), its result files, figures, examples, binaries
and Swift outputs in `data/prev-2026-11-19/`, with the third run's Rust outputs (regenerated with the
kept third-run binary, which the fourth run did not change: identical scores on the papers checked).
**Fourth run on 2026-11-18, Swift converter only, commit bd19a83**: word breaks read from the PDF
(aa7643a, 416b4b6) and text a page does not show (bd19a83); see "What the word-spacing and
hidden-text fixes changed" below. The third-run version of this file is kept as
[RESULTS-2026-11-18c.md](RESULTS-2026-11-18c.md), its Swift outputs, result files, figures, examples
and binaries in `data/prev-2026-11-18c/`. The second 2026-11-18 version of
this file (converters of a12fddc: reference lists found from their entries, a40821b: tables limited
to their own rules) is kept as [RESULTS-2026-11-18b.md](RESULTS-2026-11-18b.md), its converter
outputs, result files, figures and binaries in `data/prev-2026-11-18b/`; the first 2026-11-18
version as [RESULTS-2026-11-18.md](RESULTS-2026-11-18.md), with `data/prev-2026-11-18/`. The
2026-11-17 version of this file is kept as [RESULTS-2026-11-17b.md](RESULTS-2026-11-17b.md), the
previous run (2026-11-16, commit cf38874) as [RESULTS-2026-11-16.md](RESULTS-2026-11-16.md); its
result files are in `data/prev-2026-11-16/`. How to reproduce, and what each metric means exactly:
[README.md](README.md). Full tables: [results/summary_tables.md](results/summary_tables.md) and
[results/summary_extra.md](results/summary_extra.md); all numbers: [results/summary.json](results/summary.json),
[results/summary_extra.json](results/summary_extra.json); per-paper rows: `results/per_paper.csv`,
`results/per_paper_extra.csv`, `results/pages.csv`, `results/highlights.csv`,
`results/highlights_pages.csv`, `results/pdffig.csv`, `results/pairs.csv`; figures: `figures/*.pdf`.

## Headline numbers

Main corpus: 200 PMC open-access articles (CC BY/CC0), 200 journals, 51 named publishers, 2004–2026,
2,393 PDF pages. All converters produced an EPUB for all 200 papers. Medians with IQR; [ ] = 95 %
bootstrap CI (of the mean, unless stated otherwise).

| | Swift, current | Swift, baseline (38d2bdc^) | Rust (PDFium) |
|---|---|---|---|
| Token F1 vs XML, median (IQR) | **0.955** (0.918–0.980) | 0.890 (0.798–0.948) | 0.960 (0.912–0.982) |
| Token F1, mean [CI] | 0.927 [0.914, 0.940] | 0.860 [0.841, 0.877] | 0.932 [0.919, 0.944] |
| Token recall of abstract+body, median | **0.988** | 0.966 | 0.993 |
| Token precision, median | 0.940 | 0.861 | 0.939 |
| Word error rate, median | 0.087 | 0.221 | 0.079 |
| Broken words / 1000 tokens, mean [CI] | **0.79** [0.45, 1.28] | 4.15 [2.65, 5.93] | 0.46 [0.25, 0.71] |
| Paragraph order, Kendall's τ, median (mean) | 1.000 (0.986) | 1.000 (0.982) | 1.000 (0.989) |
| Papers with ≥ 1 interleaving event | **20.5 %** [15.0, 26.0] | 34.0 % [27.5, 40.5] | 12.5 % [8.0, 17.5] |
| Papers with any boilerplate leak | **41.0 %** [34.0, 48.0] | 63.5 % [56.5, 70.0] | 41.5 % [35.0, 48.5] |
| Figure images, count-based recall (precision) | 96.0 % (72.7 %) | 96.2 % (62.5 %) | 96.0 % (72.5 %) |
| Figures identified (image + matching label/caption) | 66.0 % [58.6, 73.0] | 65.7 % | 72.2 % [65.6, 79.0] |
| Highlights re-anchored correctly in the XML view | **89.4 %** [87.6, 91.0] | 82.9 % [80.6, 84.9] | 90.5 % [88.8, 92.0] |
| Tables identified (image + matching label/caption) | 65.9 % [58.5, 72.9] | 56.0 % | 66.7 % [59.2, 73.8] |
| Papers with XML references but no reference items | 2 (40 in the first 2026-11-18 run) | – | 2 (40) |
| Speed, s/page (one conversion at a time), median | 0.124 (CPU 0.085) | 0.190* (CPU 0.085*) | 0.047 (CPU 0.046) |

\* baseline timed only in the 2026-11-16 batch run (3 conversions in parallel; its outputs were reused
in this run, same binary); in that run the Swift CLI of the day took 0.199 s/page wall. Wall times in
this run are about 30 % shorter than on 2026-11-16 for both converters, but the 2026-11-16 and current
`p2e` binaries took the same time when run side by side on four slow papers (e.g. 1.70 vs 1.46 s,
1.33 vs 1.27 s, 1.06 vs 1.05 s, 1.40 vs 1.39 s): the difference is load on the machine, not code.

**What the symbol-font fix changed (2026-11-20, sixth run vs fifth run; same papers, same
scripts).** Prompted by the author's Cell paper (Kukanja et al. 2024), whose reading view showed the
Highlights as "d ISS elucidates … d Active EAE lesions …", the bullets a superscript "d", and the
footer as "April 11, 2024 ª 2024 The Author(s)". Cause: Elsevier's and Springer's typesetters (Advent)
set bullets, ©, Greek and math in "Pi" and symbol fonts whose glyphs are named like letters, with no
ToUnicode map or one that only restates the names (AdvPSMPi6 draws the bullet as "d"; AdvPSSym the ©
as "ordfeminine" at 0xAA, its map `<aa> <00AA>`). PDFKit and PDFium read the names. Fixes (Swift
02a1e7f, `PDFSymbolFonts.swift`, `PDFContent.swift`, `PDFAnalyzer.swift`; Rust e1e82f0,
`core/src/convert/pdf/symbols.rs`, `extract.rs`, `layout.rs`):

- *Tables per font family, from the glyphs themselves.* Every glyph of the symbol fonts without a
  ToUnicode map in the 200 PDFs and the 40 library PDFs was drawn from the embedded font program and
  read by eye: AdvPSSym and AdvP697C follow Symbol's layout under StandardEncoding's names and "Cnnn"
  (Symbol's code: "C211" ©); AdvP4C4E74 is TeX's cmsy (OMS layout: "C2" ×, "C0" −, "exclam" →) with
  ( ) + = as "eth" "Thorn" "thorn" "onequarter" (the "ð1Þ" of Springer and Elsevier equations, as in
  TeX CM Maths Symbols); Elsevier's Mathematical Pi and Universal Greek with Math Pi name theirs
  "H9251" α … "H9262" μ and "H11001" +, "H11002" −, "H11003" × …; AdvPi1/2/3 hold Greek, ×, •, the
  legend shapes □ ○ △ ■ ● ▲ and Springer's © and ®; AdvTir_symb has × at "nine" and + at "question";
  AdvPSMP4/10/13 and AdvPS3F4C13 set Greek by Latin letter ("150 ll" for 150 μl). A name no table has
  stays as it is.
- *Swift.* The content scanner gives each code of such a font its text, by the Differences name or the
  base encoding (also when the font's ToUnicode map gives the code only what its name or code stands
  for). The glyph replaces the character PDFKit read for it (that character, starting where the glyph
  starts, on its baseline: "d", "ª", "Ó" for "C211"; put back in order where PDFKit read it out of
  order), or goes in between the letters either side where PDFKit read nothing ("239 240Pu" →
  "239+240Pu", "2 10 5" → "2×10 5"), never into a word whose letters leave no room for it; it takes the
  height of the letters next to it (no superscript bullet).
- *Rust.* PDFium gives such a glyph its name's character, so the tables go by that character; a name
  without one ("C211", "H11001") PDFium gives as its code, which says nothing without the font's
  Differences (pdfium-render has no access to them): these glyphs are still missing in the Rust view.
  For AdvP4C4E74, which often has a right ToUnicode map giving "(" or "=", only its Latin-1 stand-ins
  are mapped.
- *Lists (both).* A line starting with a bullet (• ● ▪ ◦ ‣) starts a paragraph, the item's lines
  indented to its text go on with it, and paragraphs that start with one become a list (`<ul>`).
- *Gutter (both).* A page with no two lines side by side took the middle of the page as its column
  gutter; on Cell's summary page (graphical abstract over the Highlights on the left, authors on the
  right) that cut through the longer Highlights lines and put the first item before the authors and
  the rest after "In brief". Such a page now takes the middle of the widest stretch the fewest blocks
  cross. The Highlights are now one list of four items after "Highlights", then Authors,
  Correspondence and In brief, in both converters.

Stray symbol-font glyphs (counted in the views: "ª", "Ó"/"#"/"&" before "The Author(s)" or a year,
"ð", "Þ", "¼", "þ", a superscript "d" bullet, "5 9 10"-style products): **Swift 223 → 111 in 59 → 36
papers, Rust 143 → 24 in 43 → 20 papers**; most of what is left are Swift's math in a font with its
own encoding and Rust's "Cnnn"/"Hnnnnn" glyphs, which it still drops. Token F1 medians unchanged
(Swift 0.955, Rust 0.960), Swift better in 25 papers (20 by more than 0.0005; most PMC1974791 +0.018,
"lM" → "μM", "a,b-meATP" → "α,β-meATP"; PMC5270734 +0.013), worse in 2 by at most 0.001; Rust better in
21, worse in 2 (PMC6083737 −0.029: a table set as text read in another order after the gutter change;
PMC3549477 −0.001). Broken words Swift 0.80 → 0.79, Rust 0.47 → 0.46 per 1000; papers with a leaked
boilerplate line 41.5 → 41.0 % (Swift), 42.0 → 41.5 % (Rust): fewer leaks in 5 papers each (licence
lines and running footers whose "©" now makes them alike on every page, page numbers), more in
PMC2747712 for both (its "© 2004 Cancer Research UK" line, before "& 2004", now caught by the licence
detector) and in 2 for Swift (one page number each in PMC2712078 and PMC5315376, whose equations now
read differently). Highlights → XML: Swift 88.9 → 89.4 %, Rust 90.6 →
90.5 %; fifth run's views → this run's 99.2 % (Swift) and 99.3 % (Rust), the lost ones on the text
the fix changed ("lM" now "μM"). Rust display equations kept as images 131 → 135.

40 PDFs from the author's library, old vs new views: stray glyphs Swift 183 → 120 in 12 → 11 PDFs (most left in the equations of two PDFs set in TeX CM Maths Symbols, 47c2d86f and 6cde3fb6; not traced further), Rust 188 → 16 in
9 → 4 PDFs. Changes checked by eye: Cell's and Neuron's Highlights (471f70fb) and 7f1b8a38's bullets
became lists (Swift and Rust 738/752 "•" now list items); ©, ™, ®, μm, α, ×, ∼ in R formulas
("count ∼ lesion_type"), (1)…(6) and = in equations. Wrong ones: in 3ab24f09 (a 2005 PDF whose
second column's text runs do not match PDFKit's glyphs) one μ lands in the wrong word gap, and a δ
next to a letter PDFKit widened over it loses its space ("hepatitisδ antigen"; the δ was missing
before). An intermediate Swift build also put glyphs inside words there ("nhαibited"); glyphs no
longer go where the letters leave no room, and the Swift run was redone.

**What the hidden-text (Rust), ligature and accent fixes changed (2026-11-19, fifth run vs fourth
run for Swift, vs third run for Rust; same papers, same scripts).** Prompted by the fourth run, after
which the Rust converter still read the hidden manuscript text of MDPI's figure pages (MDPI median
token F1 0.875 against 0.983 for Swift; "x FOR PEER REVIEW" heads in 11 papers), and by the split
words left in the author's library PDFs. Causes and fixes:

- *Rust: text a page does not show* (b25a922; `core/src/convert/pdf/extract.rs`). The port of the
  Swift fix (bd19a83) with PDFium: walking the page's objects, a text object inside a form XObject is
  hidden when its middle lies outside its clip (PDFium clips each object in a form to the form's box,
  and leaves out the clip of an object wholly inside it) or under an image a form draws after it;
  PDFium's characters are matched to their text objects by the objects' bounds and the hidden ones
  dropped (the line around them re-decided from where the letters stand); images and drawings count
  only inside the box. An image covering the whole page hides nothing (it may be a scan drawn over its
  own text layer; the same guard added in Swift).
- *Swift: ligatures PDFKit leaves out* (58eaa6d; `PDFContent.swift`, `PDFAnalyzer.swift`). A font
  that puts "fi"/"fl" at codes below 32 and names them in its encoding ("/uniFB01", with a ToUnicode
  map to U+FB01) loses them in PDFKit's text ("detoxi cation", "to lter", "we nd"; PDFium reads them).
  The content scanner records where such glyphs are drawn; their letters go where PDFKit has only a
  space or nothing and the glyphs leave room (a symbol a wrong ToUnicode map gives, "di↵erent", is
  replaced; a code whose ToUnicode entry says something else is not a ligature), and a line PDFKit
  broke at the ligature goes on without a space.
- *Accents* (58eaa6d Swift, 7dd5b88 Rust). Elsevier's accent font (AdvP4C4E59) draws the dieresis at
  the code of "€" ("J€akel", "M€achler", "Br€uck"; PDFium gave "Jakel" or "€ Jakel"), TeX draws "´"
  before its "e" ("St´ephane"), and PDFKit leaves a space after a letter and its combining accent
  ("Cance´ rologie", "Cˇ uklina") or breaks the line there ("Falca˜|o"). Swift: an accent goes on the
  letter after it when the two make a letter and the letter before and the accent do not (so
  "Mu¨nster" and the degree sign in "4˚C" stay as they were); the space after a combining accent goes
  where the next letter is closer than the line's word spaces; a line broken after an accented letter
  goes on. Rust: an accent drawn back from where the text has got to is set aside and folded over its
  letter, "€" counting as a dieresis only over a letter, and the word space at its old place stays
  where the letters either side stand apart ("UniversitätMünchen" was the first try). The two fonts'
  control-code accents (acute, ring at codes 1–3: "Gomez", "Stahl") are dropped by both libraries and
  stay unaccented.
- *Swift: a glyph PDFKit cannot map* (a Greek letter in a symbol font: "hepatitis δ antigen") widens
  the box of the letter next to it, whose middle then lay past the word gap ("hepatiti s"): word gaps
  are looked for only within 0.3 em of a letter's edge.
- *Swift: glyphs drawn twice.* A Wiley book chapter draws its bullets and line-end hyphens several
  times over; PDFKit gathered the copies into lines between the text lines ("chro-|- • - • - •|mosomes",
  "analy•|y• y• y•|ses", 90 split words). A line of nothing but copies of letters drawn before goes, a
  bullet with a box across the whole line goes, and a word goes on across them.

40 PDFs from the author's library (as in the fourth run; split = two tokens that are one word in the
other converter's text or the dictionary, wrong join = a token that is two words there; Swift measured
against the new Rust text, Rust against the new Swift text, both before and after):

| | splits before | splits after | wrong joins before | wrong joins after |
|---|---|---|---|---|
| Swift | 164 | 70 | 51 | 53 |
| Rust | 27 | 27 | 35 | 34 |

Swift by cause: the book chapter 90 → 25, ligatures 14 → 1 (4674cffe), accents 15 → 1 (471f70fb,
b715ec6d, 16aa1e63, 7a8859cd, 416b6bec, c41bfd4e), the symbol-font gap 16 → 10 (3ab24f09). The two
new joins are not new: "Calca.Weconfirmed" and "A.E.CellProfiler" are words PDFKit set without a space,
counted now that their "fi" is back. Left: in the book, hyphenated words cut by figures or page breaks
(the next line is in another paragraph) and code ("clust alw"); math ("zijx i", 7, a 2005 PDF's
sub- and superscripts; Rust has 2); URLs and a Scientific Reports author line (aecd3de9, 10); the rest
of 3ab24f09 (10: a 1990s PDF whose second column's text runs are narrower in the content stream than
PDFKit's glyphs, so gaps fall inside words; not traced further). Rust's gains on these PDFs are
accents (every "J€akel" etc. now "Jäkel"), not counted as splits.

Corpus (200 papers): **Rust** token F1 median 0.952 → 0.960 (IQR 0.890–0.977 → 0.913–0.982), mean
0.920 → 0.932; precision 0.933 → 0.939; recall 0.992 → 0.993; F1 better in 20 papers by more than
0.0005 and worse in none; MDPI's 26 papers median 0.875 → 0.985 (Swift 0.983); papers with
interleaving 16.0 → 12.5 %, with a leaked header or footer 46.0 → 42.0 %; broken words 0.48 → 0.47 per
1000; figures identified 71.7 → 72.2 % (better in 4 MDPI papers), tables identified 67.1 → 66.7 % (one
table each lost in PMC6213515 and PMC9504565, the two MDPI papers whose table Swift also lost with its
hidden-text fix); recall fell in one paper (PMC5456230, −0.006: its hidden copy held words both
converters otherwise miss; Rust's text of it is now Swift's). Highlights: Rust view → XML 89.5 →
90.6 %; third run's Rust views → this run's 98.4 % (61 of 4,000 lost, 56 in 15 MDPI papers, on the
hidden copy). **Swift** almost unchanged on the corpus, as few PMC PDFs have these fonts: token F1
median 0.955, mean 0.927, better in 17 papers by more than 0.0005, worse in none; broken words 0.82 →
0.80 per 1000 (fewer in 7 papers, more in 7, most by under 0.1; PMC12257303 +0.76: an RSC paper whose
"fi" PDFKit drops, now back in "films" but in two places as "fi lms", where its glyphs are not where
the widths put them; PDFium drops this "fi" too, "thin lms"); highlights → XML 88.9 % (unchanged),
fourth run's views → this run's 99.8 % (5 of 4,000 lost). An intermediate Swift build was worse in 5
papers (fonts whose encodings name ";" and "(" as ligatures; "4˚C" read as "4C̊", "Mu¨nster" as "Mun̈ster")
and crashed on 8 (a ToUnicode range written backwards); both were fixed and the run redone. Swift–Rust
agreement 0.989 → 0.993 (IQR 0.963–0.998), per-paper F1 correlation r 0.73 → 0.83. Rust − GROBID
token F1 now +0.006 [−0.006, +0.017] (p = 0.07; was −0.006). Not rerun: the baseline, XML and pdffig
views (no converter change affects them), speed.

**What the word-spacing and hidden-text fixes changed (2026-11-18, fourth run vs third run; Swift
only, same papers, same scripts).** Prompted by a bioRxiv manuscript from the author's library (not in
the corpus) whose reference list read "A whole-slide foundati on model for digital pa thology f rom
real -world data. Nat ure 6 30, 181-188", and by the error analysis below, where Swift had 2.8 broken
words per 1000 tokens against 0.5 for Rust, almost all in MDPI PDFs. Causes and fixes
(`apple/Paper2EpubKit/Sources/Paper2EpubKit/PDFContent.swift`, `PDFAnalyzer.swift`; Swift only, the
Rust converter takes its text and spaces from PDFium):

- *Spaces PDFKit reports without room for them* (416b4b6). The manuscript was printed to PDF in a
  Type 3 font, its text placed in pieces 15/1000 em (0.18 pt) apart; PDFKit's text has a space at
  each joint, and its one-character selections give that space a box 0.18 pt wide. The content
  scanner now records where the text goes on with no space drawn on either side (and space glyphs
  that word spacing takes all the room from, as some publishers put in URLs: "https:// creat iveco
  mmons. org/"); a PDFKit space there is dropped when the glyph boxes either side leave less than
  0.1 em. Both conditions are needed: tightly justified lines have real word spaces of 0.05–0.1 em
  (a space glyph with negative word spacing), and synthetic italics (a skewed font) widen PDFKit's
  boxes until a drawn space looks like none ("et al").
- *Word gaps measured with wrong glyph widths* (aa7643a). The scanner puts a space where the text
  jumps right by more than 0.15 em; it read a Type 3 font's widths (in the font's own units, here
  1/67 em) as thousandths of an em, so every piece seemed to end where it began ("Nat ure 6 30");
  the standard fonts named without widths (Helvetica, Times, Courier) were given 500 for every glyph
  ("R ESULTS", "0.9 15" in an old Distiller PDF, "Departmen t" in the Courier text layer of a scanned
  1982 paper). Widths now come from the FontMatrix and from the system's copy of the font, and in
  text squeezed by horizontal scaling (OCR layers, "48.53 Tz") the threshold shrinks with it.
- *Text a page does not show* (75a85ec; the "partitioni ng" splits). MDPI's pdfTeX PDFs include each
  figure as a whole page of the manuscript ("cells-2381855-g001.pdf": the figure and the
  manuscript's own text around it) cut to the figure by the form's bounding box. PDFKit reads the
  hidden text too and mixed it into the article's lines, a point away and set differently: "two
  molecules o f t he four hist one protei ns", duplicated captions, and in 26 MDPI papers 11.4 broken
  words per 1000 tokens. The scanner keeps the forms' boxes: text outside one, or under an image the
  form draws after it, is hidden, and images and drawings count only inside the box (so a figure no
  longer reaches over its caption). A letter is hidden when a hidden glyph starts where it does and
  no shown one does (by type size first, as a copy may lie within a point of the text shown); a line
  of mostly hidden letters goes whole, and spaces are re-decided where hidden letters were taken out.

Test PDFs: the bioRxiv manuscript's reference list has no split word left (before: "foundati on",
"pa thology", "f rom", "real -world", "Nat ure 6 30", "transcrip tomics", "e lucidat e int ercel l
ular"; after: "A whole-slide foundation model for digital pathology from real-world data. Nature 630,
181-188 (2024)."); a second preprint (CellBridge, arXiv style) is unchanged except that two figure
captions that had been lost are now attached. On 40 PDFs from the author's library (mixed publishers
and years, compared against the Rust converter's text: a split = two tokens that are one word in Rust's
text or the dictionary; a wrong join = a token that is two words in Rust's text): splits 335 → 175
(Rust 26), wrong joins 51 → 51 (no new one; Rust 40); four papers changed: a 1982 scanned paper
(104 → 0, its OCR layer), a scanned 1965 paper (46 → 0, the same kind of OCR layer), an MDPI paper (15 → 10, URLs) and a
Springer paper (7 → 2, URLs). The remaining splits there have other causes: a book chapter with
figure text between hyphenated lines (90), mis-decoded accents ("J€akel", 18), a figure's PDF text
in a 2005 PDF (16), a ligature "fi" PDFKit cannot map ("detoxi cation", 14), math ("zijx i").
Regression tests (`apple/Paper2EpubKit/Tests`, `swift test`): PDFs made on the fly for each case, and
the layouts whose spaces must stay (justified and tightly justified text, word gaps without space
glyphs, kerning, a letter-spaced heading, small caps, superscript citations, tight table columns,
two columns, ligatures, math).

Corpus (200 papers): broken words 2.81 → 0.82 per 1000 tokens (Rust 0.48), spurious splits 2.47 →
0.53 (3,312 → 394 in all), spurious joins 0.24 → 0.18; papers above 5 per 1000 25 → 7 (Rust 5);
MDPI's 26 papers token F1 median 0.879 → 0.983 (Rust 0.875: the Rust converter reads the hidden
text too, as extra words, see "Swift vs Rust"), broken words 11.4 → 0.0. Token F1 median 0.949 →
0.955 (IQR 0.888–0.974 → 0.918–0.979), mean 0.913 → 0.927; precision 0.920 → 0.940; recall median
0.987 (unchanged; mean 0.958 → 0.959); papers below F1 0.9 28.0 → 20.5 %; F1 better in 25 papers by
more than 0.0005 and worse in none. Recall fell in two papers: PMC5723519 (−0.018, an old Distiller PDF
whose italic variables are now set in italics, "<em>y</em>1", which splits tokens in the metric) and
PMC9504565 (−0.002). Interleaving 23.5 → 20.5 % of papers (better in 6, worse in none); papers with
a leaked header or footer 46.0 → 41.5 % (MDPI's "x FOR PEER REVIEW" heads were in the hidden text);
figures identified 65.6 → 66.0 % (better in 4 papers, worse in 1: PMC7663294, a caption no longer
paired with its image), figure captions kept 57.1 → 61.0 %, tables identified unchanged (65.9 %;
one table each lost in PMC6213515 and PMC9504565, two gained in PMC5723519). Highlights: current PDF
view → XML 87.2 → 88.9 % (lost 11.2 → 9.8 %); highlights made on the third run's views and moved to
this run's: 97.8 % (3,913 of 4,000; 70 of the 72 lost are in 17 MDPI papers, on text that was the
hidden copy or garbled by it), manuscripts 100 %; baseline view → current view 97.1 → 94.8 % for the
same reason (highlights on garbled MDPI text: 101 → 176 lost, the increase in 14 MDPI papers and one Wiley paper).
Batch speed unchanged (0.123 s/page wall, 0.087 CPU, was 0.122 / 0.085).

**What the figure and front-matter fixes changed (2026-11-18, third run vs second run; same papers,
same scripts).** Prompted by a Nature Neuroscience PDF from the author's library (not in the corpus)
whose reading view (Swift) had a "Main text" chapter holding only a picture of the first page's
"Published online / Check for updates" box, the footnote "A full list of affiliations appears at the
end of the paper. ✉e-mail: …" in the Introduction, every main figure cut into 4–6 images (39 figure
images for 8 figures, 10 Extended Data figures and 4 reporting-summary pages) and panel letters as a
heading ("a b" in the contents). Causes and fixes (`core/src/convert/pdf/layout.rs`,
`PDFAnalyzer.swift`, Swift and Rust alike):

- *Figures in pieces.* Pieces of a figure were joined only up to 22 pt apart; Nature's rows of panels,
  with their letters and axis titles between them, are further apart. Pieces are now joined up to
  60 pt apart while the joined box holds no running text (a line of more than 50 characters), caption
  or heading, and not when each piece sits over its own caption (two figures side by side), nor a lone
  shape (a frame's corner) or a logo in the page margin; pieces grown into each other by their labels
  are one figure. A caption's continuation in the other column is no longer taken for a label of the
  figure above it (one Extended Data legend was swallowed into its picture by the Rust converter), nor
  is a line of running text beside a figure.
- *First-page boxes as pictures.* "Published online: …" sits on a shaded image strip next to the
  Crossmark badge: a band behind a line of text is no picture now, and neither is the masthead above
  the title ("ARTICLE | Open Access" banners, the "Scientific Data" logo), a bar down the page's edge
  ("Author manuscript", "Europe PMC Funders Author Manuscripts"), or on the first page a frame that
  holds running text rather than a picture. In Europe PMC manuscripts the side bar joined the header
  box's rules into a picture of the whole first page, so title, abstract and introduction were lost.
- *Front-matter footnotes in the body.* The first two pages' footnotes go to the title page with the
  rest of the front matter: correspondence and e-mail, "A full list of affiliations…", dates
  ("Received …; Received in revised form …; Accepted …", "Published online: …"), editor, competing
  interests, ORCID, equal contribution and licence lines. In footnote type anywhere on the page; in
  body type only at the foot of the page and when the paragraph starts like a footnote ("*
  Correspondence:", "Received 12 May"); Data and Code availability are never taken.

Test paper: 39 → 22 figure images, one per figure (the four reporting-summary pages stay one picture
each), the Extended Data figures whole and captioned, no "Main text" chapter and no panel-letter
headings (contents: Abstract, Introduction, Results, Discussion, … as in the PDF); dates and footnote on
the title page. Corpus (200 papers + 30 manuscripts): figure images 1,223 → 1,128 (Swift) and 1,241 →
1,131 (Rust) for 849 figures (count-based precision 66.7 → 72.3 %, 65.8 → 72.1 %; exact figure count in
39.5 → 45.0 % of papers), manuscripts 210 / 245 → 168 for 159; figures identified 63.8 → 65.6 %
(Swift), 71.4 → 71.7 % (Rust); tables identified 63.4 → 65.9 %, 66.9 → 67.1 % (better in 11 / 1
papers, worse in none); a "Main text" chapter (material before the abstract) in 101 → 47 (Swift) and
101 → 45 (Rust) of 230 papers, first-page figure images 120 → 53 and 121 → 52. A scan of each view's
first quarter of body paragraphs for footnote cues (e-mail address, "Correspondence", "Received …
Accepted", licence and copyright lines) found them in 81 → 13 (Swift) and 79 → 14 (Rust) of 230
papers; of the remaining, 3 are STAR Methods "lead contact" paragraphs (body text) and 3 are on first
pages that were pictures before (PMC2702416, PMC3013247, PMC8823923); 115 (Swift) / 103 (Rust)
paragraphs in 71 / 69 papers moved from the body to the title page, all of them read and all front
matter (Swift: correspondence or e-mail 50, licence and copyright 45, dates 11, competing interests 6,
editor, equal contribution, an address); papers with a leaked header, footer or licence line
54.0 → 46.0 % (Swift), 53.0 → 46.0 % (Rust). Token F1 median 0.944 → 0.949 (Swift), 0.949 → 0.952 (Rust), mean 0.907 → 0.913, 0.914 →
0.920; precision 0.916 → 0.920, 0.929 → 0.933; recall median 0.986 → 0.987, 0.991 → 0.992 and mean
0.953 → 0.958, 0.966 → 0.971: text came back in 12 papers whose first page (or more) had been a
picture (recall +0.03 to +0.26: PMC7611092 0.93 → 1.00, PMC10754574 0.85 → 1.00, PMC3013247 0.00 →
0.26, PMC2702416 0.30 → 0.47), and fell by more than 0.002 in one (PMC5903372, −0.004, its licence and
date lines). F1 better in 62 (Swift) / 61 (Rust) of the 200 papers, worse in 1 / 3, by at most
0.005. Inspected: figures identified fell by one in 3 papers per converter (PMC2945643, PMC3354641,
PMC7085885, PMC6083737); in each, pieces of one multi-panel figure are now one image, as they should
be (the scoring's pairing of images with labels was not traced further); PMC8823923 gained two interleaving events (23.0 →
23.5 % and 15.5 → 16.0 % of papers), because its first page, a picture before, is now text, read in
the wrong order in one place. Author manuscripts: median F1 0.978 → 0.988 (Swift), 0.982 → 0.990
(Rust). Highlights: unchanged for Swift (87.2 %; now 4,000 highlights in 200 papers, PMC3013247 having
text), Rust 89.2 → 89.5 %, manuscripts 96.0 → 96.5 %.

**What the reference-list fix changed (2026-11-18, second run vs first run; same papers, same
scripts).** In the first run 41 of the 200 papers had no reference items: no stand-alone "References"
heading was found, so the list stayed in the body text (median token F1 0.786 Swift / 0.792 Rust,
GROBID 0.968 on the same papers). Looking at their analysed blocks (`P2E_DEBUG=1`): 27 had the heading
on the first line of the list's first paragraph ("References 1. Hanahan D. …", BMC and Springer: the
heading in the reference type, its font name not marked bold); 8 had a bold "References" in the small
reference type (7.5 pt against a 10 pt body, so the layout analysis rejected it as a heading); 2 had an
ornament before an all-caps heading ("❚ REFERENCES", "■ REFERENCES"); 3 had no heading at all (Wiley
"[1] Y. Guo, …" lists after the Keywords and dates, and Nature's two numbered lists, the main one before
Methods); 3 could not be fixed (PMC3013247: no text, every page an image, now its first page has text; PMC2702416: most pages
images; PMC8282988: no `ref` in the XML, open peer-review reports instead). The converters now
(`core/src/convert/pdf/refs.rs`, `PDFReferences.swift`):

- take a heading the layout missed when entries follow it: "References" in small type, on the first
  line of the list, with an ornament, numbered ("6 References"), letter-spaced, or "Literature Cited",
  "References and Notes", "Bibliography", "Literatur", "Références", "Referencias" and a few more;
- without any reference heading in the paper, take a run of ≥ 6 paragraphs that start like a
  reference (a number "1." "[1]" "1 Smith", or an author "Smith JA," "Smith, J. A.," "J. A. Smith,")
  and look like one (score ≥ 3 of: year; year in brackets or before volume; volume and pages; DOI or
  web address; "et al."; two or more initials; minus 2 for running text), numbered in order (≥ 80 %
  of steps +1…+5) or sorted by author, after the first 30 % of the text, starting on page 2 or later,
  with at most two paragraphs of body-size running text before the next section;
- cut the list into entries: a numbered list by its numbers (a line with the next number, or a number
  at the hanging indent, starts an entry; anything else continues the current one, across columns and
  pages; a number on a line of its own goes with its entry), an author–year list by paragraph, at the
  hanging indent, or after a short line that ends an entry;
- emit `<ol class="refs" style="list-style-type: none; padding-left: 0">` with one `<li>` per entry,
  numbered as printed, which the References panel already reads (`apple/Shared/References.swift`).

Result: 38 of the 41 lists are found (all three that could not be are listed above); median token
F1 on these 41 papers 0.786 → 0.946 (Swift), 0.792 → 0.957 (Rust). Over all 200: Swift 0.913 → 0.944,
Rust 0.912 → 0.949 (GROBID 0.947); precision 0.873 → 0.916 (Swift), 0.869 → 0.929 (Rust); recall
unchanged (0.984 → 0.986). Papers without reference items 41 → 3 (both converters), 40 → 2 of those
with references in the XML. Reference items within 10 % of the XML's count: 67 → 168 papers (Swift),
88 → 155 (Rust) of 199. The heading-less rule fired in exactly the 3 heading-less papers (checked by
logging every run it accepted over all 230 papers); no body text was taken for references (every
paper whose F1 fell by more than 0.005 was inspected: all from tables, below). Author manuscripts:
median F1 unchanged (0.978 Swift, 0.982 Rust), every list found as before.

**What the table fix changed (same run).** A table on a two-column page (GPT-2's Table 1, a box of
translations in the right column between two rules) was cut together with the running head's rule
above it, so the image spanned both columns and the left column's text appeared twice. A table's
rules are now those about as wide as the rule next to its caption (0.6–1.5×, overlapping it), a
table ends before another table's or figure's caption, and a rule drawn in pieces (one per column of
the table) is one rule; if that leaves a table a single rule, the old grouping is used. Tables
identified: Swift 268 → 307, Rust 287 → 324 of 484 (better in 33 papers, worse in 1 Swift and 0
Rust); figures unchanged (542 and 606). Cost: in 6 (Swift) / 7 (Rust) papers token F1 fell by 0.006–0.024,
because a table that the previous table's rules used to swallow in one image is now cut only between
its own rules and some of its rows leak as text (e.g. PMC9105103's Table 3, whose header and bottom
rules are 254 pt apart, more than the 250 pt allowed between two rules of one table).

**PDF fixes since the baseline (current − baseline, paired over 200 papers):** token F1 +0.067 [+0.056, +0.079]
(Wilcoxon p = 6e-28; better in 171 papers, worse in 18, unchanged in 11; +0.053 before the
word-spacing and hidden-text fixes, +0.047 before the figure and front-matter fixes, +0.017 before the
reference-list fix); recall +0.034 [+0.027, +0.042]; WER −0.147 [−0.176, −0.120]; spurious intra-word
splits −3.34 per 1000 tokens [−5.04, −1.84] (−1.39 before the word-spacing fixes); interleaving events
−1.11 per paper [−1.67, −0.57] (p = 2e-5); paragraph order τ +0.004 [+0.000, +0.009] (p = 4e-4);
boilerplate leaks −0.78 per paper [−1.03, −0.56] (p = 2e-12), mostly publisher/licence lines and
"(legend continued on next page)" and running headers/footers (−0.19); papers with any leak 63.5 % →
41.5 %. Reference-list items recovered (median ratio to XML) 0.85 → 1.00. Tables identified 56.0 % →
65.9 %; highlight survival 82.9 % → 88.9 %. Unchanged within noise: figure detection (65.7 % → 66.0 %
identified, but 1,308 → 1,121 images for 849 figures), lost compound hyphens.

**What the 2026-11-17 fixes added (this run vs the 2026-11-16 run, same papers):** column reading
region by region (three columns, boxes, full-width figures with two-column legends) and STIX text fonts
no longer taken for math. Token F1 +0.003 on average for both Swift (60 papers better, 17 worse) and Rust
(56 better, 16 worse); papers with interleaving 25.5 % → 23.0 % (Swift), 18.0 % → 15.5 % (Rust);
equation images 198 → 133 (Swift; Wiley papers 39 → 14), as running text in STIX fonts is no longer
cropped as equations; highlight survival 83.7 % → 84.2 %.
One small regression: leftover line-break hyphens ("transfor-mation") +0.07 per 1000 tokens
[+0.02, +0.13] (22 papers worse, 5 better), from lines whose hyphen is now kept as a compound hyphen.

**Swift vs Rust:** the two implementations agree closely — token F1 between their outputs median
0.992 [0.989, 0.995] (IQR 0.963–0.998; 0.993 in the fifth, 0.989 in the fourth and third, 0.988 in the second and 0.990 in
the first run); their per-paper F1 vs XML correlates at r = 0.83 (0.73 in the fourth run, when Rust
still read the hidden manuscript text of MDPI's figure pages; MDPI median F1 now 0.985 Rust, 0.983
Swift). Both found a reference list in the same 197 papers. Rust is level on precision (+0.000
[−0.011, +0.010]) and F1 (+0.005 [−0.003, +0.013]), ahead on recall (+0.013 [+0.007, +0.019]), broken
words (−0.33 per 1000 tokens [−0.77, −0.06]), interleaving (−2.0 events per paper) and figure
identification (+6.2 points), behind on page-number leaks (+0.14 per paper), and is 2.6× faster per
page (PDFium vs PDFKit).

**Figures cut from the PDF for an XML version without images:** for the 30 NIHMS author manuscripts,
96.9 % [90.8, 100] of the XML's figures were recovered from the PDF (all figures in 28/30 papers,
none in 1); spot-checked images were the right figures. Applied to the 200 publisher PDFs the same
label-based cutting recovers only 49.1 % [40.5, 57.5] — publisher layouts (figures on separate pages
or at the end, captions apart from images) are harder than the uniform manuscript layout.

**Highlight survival across reading-view versions** (20 simulated highlights per paper, located by
the reader's own `locate()` from `apple/Shared/Highlights.swift`):

| Highlight made on → re-anchored in | exact | whitespace fallback | quote/dash/case fallback | start/end fallback | lost | anchored correctly | anchored to wrong text |
|---|---|---|---|---|---|---|---|
| current PDF view → XML view | 73.0 | 6.6 | 8.5 | 2.6 | 9.3 | 89.4 [87.6, 91.0] | 1.3 |
| baseline PDF view → XML view | 66.3 | 5.1 | 9.9 | 3.8 | 14.9 | 82.9 [80.6, 84.9] | 2.3 |
| Rust PDF view → XML view | 76.0 | 5.8 | 7.6 | 2.0 | 8.6 | 90.5 [88.8, 92.0] | 0.9 |
| baseline PDF view → current PDF view | 90.0 | 0.7 | 3.3 | 0.9 | 5.0 | 94.1 [92.4, 95.6] | 0.8 |
| fifth run's PDF view → current PDF view (Swift) | 98.7 | 0.0 | 0.2 | 0.4 | 0.7 | 99.2 | 0.1 |
| fifth run's PDF view → current PDF view (Rust) | 99.0 | 0.0 | 0.2 | 0.1 | 0.6 | 99.3 | 0.1 |
| third run's Rust PDF view → current Rust PDF view | 98.0 | 0.0 | 0.1 | 0.3 | 1.5 | 98.4 | 0.1 |
| author manuscripts: current PDF view → XML view | 89.3 | 4.3 | 2.0 | 1.2 | 3.2 | 96.5 [94.7, 98.2] | 0.3 |

The fallbacks matter: they rescue 17 of every 100 highlights (whitespace 6.4, fold 8.3, start/end
2.7) when a paper moves from its PDF reading view to the XML one, at a cost of 1.3 % mis-anchored.
Highlights made on the baseline's view survive the regenerated PDF view in 94.7 % (94.8 % in the fourth
run, 97.1 % in the third): since the fourth run the views of 15 MDPI papers lack the hidden manuscript
text and its garbling, so highlights on it have no counterpart; from the fourth run's views 99.8 %
survive, and from the third run's Rust views 98.4 % (the lost ones in 15 MDPI papers, on the hidden
copy) (`results/highlights_update.csv`, `node s04_highlights.mjs --scenarios
swift-prev:swift,rust-prev:rust --out highlights_update` with `data/out/swift-prev` and
`data/out/rust-prev` linked to `data/prev-2026-11-19/out/swift` and `.../rust`).
(Since the figure and front-matter fixes 4,000 highlights in 200 papers: PMC3013247, whose first page
was a picture before, now has text to place them in.)
(Before the reference-list fix: 84.2 % correct, 14.3 % lost; random highlights are drawn from body
paragraphs, which then included reference entries that have no counterpart in the XML view's body.)

## Outside reference points: pdftotext and GROBID (2026-11-18)

The same 200 PDFs, scored with the same functions as the converters (`s07_baselines.py`,
`s08_extra_metrics.py`, `s09_extra_aggregate.py`). **pdftotext** (poppler 26.10.0, default reading
order) gives all text of the PDF and is the floor; `-layout` is worse (it keeps columns side by side)
and is listed only for completeness. **GROBID** 0.9.1 with its CRF models (Docker image
`grobid/grobid:0.9.1-crf`, native arm64, container limited to 6 GB and 3 CPUs, one PDF at a time,
`processFulltextDocument` without consolidation): the TEI abstract, body and back-matter divisions
(headings and paragraphs) without the reference list, figures, tables or formulas, i.e. the same
selection as the ground truth. GROBID gives no figure images, so the figure rows do not apply to it.
GROBID's deep-learning models (the `-full` image) are amd64-only and were not run.

| | pdftotext | GROBID 0.9.1 (CRF) | Lunta Swift | Lunta Rust |
|---|---|---|---|---|
| Token F1, median (IQR) | 0.716 (0.658–0.767) | 0.947 (0.895–0.974) | 0.955 (0.918–0.980) | **0.960** (0.912–0.982) |
| Token F1, mean | 0.702 | 0.926 | 0.927 | 0.932 |
| Recall, median | 0.975 | 0.985 | 0.988 | 0.993 |
| Precision, median | 0.570 | 0.930 | 0.940 | 0.939 |
| Broken words / 1000, mean | 0.87 | 0.43 | 0.79 | 0.46 |
| Papers with interleaving, % | 60.5 | 14.5 | 20.5 | 12.5 |
| Papers with any boilerplate leak, % | 100.0 | 47.0 | 41.0 | 41.5 |
| Figures identified (image + label), % | – | – | 66.0 | 72.2 |
| Papers with XML references but no reference items found | (no split) | 0 | 2 | 2 |
| s per page, median | 0.005 | 0.173* | 0.124 | 0.047 |

\* GROBID wall time per request to the warm local server (Docker VM, 3 CPUs), not comparable one to
one with the CLI times (process start included).

- **Lunta is now level with GROBID** on token F1 (medians 0.955 Swift, 0.960 Rust, 0.947 GROBID):
  Swift − GROBID mean +0.001 [−0.012, +0.013] (Wilcoxon p = 0.15; Swift better in 106 papers, worse
  in 94; −0.013 [−0.025, −0.002] in the third run, before the word-spacing and hidden-text fixes);
  Rust − GROBID +0.006 [−0.006, +0.017] (p = 0.07; better in 108; −0.006, p = 0.81, before the Rust
  hidden-text fix). (Second 2026-11-18 run, before the
  figure and front-matter fixes: −0.019 [−0.033, −0.006], p = 0.06, and −0.012.) GROBID keeps a lead
  in the papers with appended material and the three with little usable text.
- **The gap was the reference list.** In the first 2026-11-18 run GROBID was ahead (Swift − GROBID
  −0.049 [−0.065, −0.033], p = 2e-7): in the 159 papers in which the Swift converter found reference
  items, median F1 was 0.942 (Swift), 0.938 (Rust) and 0.941 (GROBID); in the 41 in which it did not
  (the list stayed in the body text), 0.786, 0.792 and 0.968. GROBID's sequence labelling separates
  the bibliography without relying on a "References" heading; Lunta now finds the list from its
  entries too (above), and on those 41 papers reached 0.946 (Swift) and 0.957 (Rust) in the second run.
- pdftotext keeps almost all words (recall 0.975) but adds everything else on the page (precision
  0.570: running headers, affiliations, captions, tables, references), and 60 % of its outputs have
  interleaved paragraphs (floats and side columns read into the text).
- XML reading view (control) on the same scoring: F1 1.000, 0.5 % of papers with a leak, 1 % with
  an interleaving event (unchanged).

## Reading views from the journal's article page (2026-11-18)

"Use this page's full text" converts the article page as the browser shows it. To test the page
readers without a browser or an account, open-access papers of the main corpus were fetched from
their publisher (`s06_pages.py`): the DOI resolved through doi.org with a plain GET, a Safari
User-Agent, cookies kept, no JavaScript, no login, one request every 6 s, robots.txt checked
(user agent `*`). A page behind a bot check, an HTTP error or a robots.txt that could not be read or
disallowed the path was recorded and skipped; nothing was done to get around a block. The figure
images that the page shows were then fetched once each (`s06b_page_images.py`,
`s06b2_page_images.py`, 2.5 s apart) and passed with `--images`, as the extension would send them;
the pages were converted with `p2e --page` and `paper2epub --page` of commit 86e764b
(`s06c_convert_pages.py`). **This measures the page readers, not the access path**: the pages were
open access and fetched without a login.

**Fetching.** First draw: 48 papers from the publishers with a dedicated reader or a large share of
the corpus (6 BMC, 6 Nature Publishing Group, 3 Springer, 6 Wiley/Hindawi, 6 PLOS, 6 Frontiers,
6 MDPI, 3 OUP, 4 Elsevier/Cell Press, 1 eLife, 1 PeerJ; seed 20261116). Because most of these
platforms refused the request, a second draw tried every other main-corpus paper with a DOI on
another platform (44 papers). Of the 92 papers:

- **55 not obtained**: HTTP 403 from bot protection (Cloudflare, Akamai) 34 — all of Wiley/Hindawi,
  MDPI, OUP (academic.oup.com), Elsevier (ScienceDirect, cell.com, thelancet.com, jbc.org), ASM,
  ACS, SAGE, Taylor & Francis, LWW, BMJ, PeerJ, Cureus, RSC, IUCr, Impact Journals; link.springer.com
  (all BMC, Springer and EMBO J papers, 10) served the article but answered robots.txt with a
  JavaScript "Client Challenge", so its permission could not be checked and the pages were not used;
  robots.txt disallowed or unreadable 3 more (Thieme, Ovid, IJCCM); JavaScript challenge 4 (JMIR × 3
  with HTTP 202, eLife); network or certificate errors 3; one DOI leads to a PDF.
- **7 saved pages without the full text** (Pensoft × 2, ecancer, SciELO Dement Neuropsychol, Aging
  and Disease, Arch Endocrinol Metab, einstein): the article text is loaded by scripts or sits in a
  PDF (none of the XML's body paragraphs occurs in the saved HTML). **Lunta refused all seven** as
  abstract-only pages ("This page only shows the abstract …"), as designed.
- **30 converted**: nature.com 5 (Springer Nature reader), PLOS 6, Frontiers 12, and 7 others by the
  general reader (CDC Emerging Infectious Diseases, F1000Research, Gates Open Research, SciELO × 2,
  Beilstein J Nanotechnol, Spandidos); 2009–2024. The dedicated readers for Atypon, Silverchair,
  Cell Press and ScienceDirect could not be exercised. Per paper: `results/pages.csv`.

The Swift and Rust page readers produced the same text for all 30 pages (identical F1). Same 30
papers, all views scored against the XML:

| | page view | PDF view (Swift) | PDF view (Rust) | GROBID | pdftotext | XML view |
|---|---|---|---|---|---|---|
| Token F1, median (IQR) | **0.956** (0.943–0.968) | 0.909 (0.833–0.957) | 0.934 (0.866–0.969) | 0.938 (0.889–0.964) | 0.628 | 1.000 |
| Recall, median | 0.995 | 0.973 | 0.978 | 0.967 | 0.903 | 1.000 |
| Precision, median | 0.924 | 0.862 | 0.909 | 0.928 | 0.484 | 0.999 |
| Broken words / 1000, mean | 0.05 | 0.55 | 0.46 | 0.74 | 0.64 | 0.01 |
| Papers with interleaving, % | 0.0 | 26.7 | 10.0 | 16.7 | 66.7 | 0.0 |
| Papers with licence/copyright lines in the text | 21 | 14 (second run) | – | – | – | 0 |
| Figures identified (image + label), % | 82.4 (112/136) | 84.6 | 85.3 | – | – | – |
| Reference items / XML refs, median | 1.00 | 1.00 | 1.00 | 1.00 | – | 1.00 |
| Conversion time per paper, median (s) | 0.33 (Swift), 0.30 (Rust) | | | | | |

- Page − PDF view (Swift) token F1: mean +0.048 [+0.012, +0.082] (Wilcoxon p = 4e-3), better in 23
  of 30 papers (+0.052 before the figure and front-matter fixes, PDF median 0.900; +0.063 before the
  reference-list fix, 0.880). By family (page / PDF median F1): nature.com 0.986 / 0.918, PLOS 0.938 /
  0.908, Frontiers 0.956 / 0.844, others 0.964 / 0.926.
- **Figures**: 106 of 109 XML figures outside one Nature paper; that paper (PMC8494646) has
  21 Extended Data figures that nature.com shows on separate pages, so only its 6 main figures were in
  the page (6/27). The other 3 missing are the Spandidos paper's (PMC4079434), whose figures sit in a
  side panel of thumbnails outside the article text, which the general reader does not take.
  Captions are attached to their figures.
- **Tables** are real HTML tables. Their caption is attached on nature.com (`p.table-caption`,
  all identified) but not on Frontiers and Gates Open Research pages, where it follows the table as
  a plain paragraph or becomes a heading (tables identified by label 8.4 % overall; the caption text
  itself is present). A reader improvement, not counted in the text metrics.
- **Remaining extra text**: the licence and copyright paragraphs at the end of the page (Frontiers,
  PLOS; "Papers with licence/copyright lines" above), one F1000Research paper whose page carries the
  open peer-review reports (F1 0.42, as for its PDF), and the Extended Data legends of the Nature
  paper (precision 0.67). The PDF running-header and page-number detectors do not apply to pages.
- **Reference lists** are linked entries in every page view (median 1.00 of the XML's references).

**Highlights** (`s04_highlights.mjs --scenarios … --out highlights_pages`; same 20 random highlights
per paper as in the main test):

| from → to (30 papers, 600 highlights) | exact | spacing | folded | ends | lost | correct [CI] | wrong place |
|---|---|---|---|---|---|---|---|
| PDF view (Swift) → page view | 69.5 | 8.5 | 6.3 | 4.8 | 10.8 | **88.0** [84.5, 91.2] | 1.2 |
| PDF view (Rust) → page view (Rust) | 73.5 | 8.0 | 5.7 | 2.3 | 10.5 | 88.5 [84.3, 92.3] | 1.0 |
| PDF view (Swift) → XML view, same papers | 64.0 | 9.8 | 8.3 | 4.8 | 13.0 | 86.2 [81.7, 90.2] | 0.8 |
| XML view → page view | 86.2 | 8.2 | 4.2 | 0.5 | 1.0 | 98.8 [97.7, 99.7] | 0.2 |

The replace rule ("every highlight must be found", `PageFullText.swift`), applied to k highlights
drawn from each paper's 20 (share of papers that would pass, k = 1/3/5/10/20): PDF view → page view
0.89 / 0.72 / 0.58 / 0.37 / 0.17 (PDF → XML, all 200 papers: 0.91 / 0.77 / 0.66 / 0.48 / 0.28;
XML view → page view 0.99 / 0.97 / 0.95 / 0.91 / 0.83).

## Setup

- **Converters.** Swift `Paper2EpubKit` CLI `p2e` and Rust `paper2epub` of commit a034bbc (third
  2026-11-18 run; the second used a12fddc, the first runs commit 86e764b, version 1.0.4, whose PDF path
  is unchanged up to a40821b; the `--pdf` flag is committed); the same CLI at `38d2bdc^` = b813981 built from a temporary
  worktree (removed afterwards), and the Rust `paper2epub` (core, release build, PDFium chromium/8152).
  All run with `--offline` on the PDF. The XML reading view is `p2e article.xml`. Both Swift CLIs
  were built with `xcodebuild -scheme p2e -configuration Release` from `apple/Paper2EpubKit`.
- **Ground truth.** The JATS XML of the same article version: abstract + body paragraphs and section
  titles (figures, tables, display formulas and footnotes excluded; non-reference back matter
  allowed in the output without penalty), `fig` with a graphic, `table-wrap`, `disp-formula`, `ref`.
  Scoring the XML reading view with the same metrics gives F1 0.999 and near-zero error rates, so
  the pipeline itself adds almost no noise (residual: 0.135 "joins"/1k from XML-internal word forms).
- **Runs.** 3 conversions in parallel for all metrics (2 in the second and third 2026-11-18 runs; `summary_tables.md` still says 3); speed from a separate pass converting 60 papers
  one at a time; per-process wall clock, CPU (user+sys) and peak memory.

## Corpus

Sampling frame: the S3 inventory of the PMC Article Datasets (`s3://pmc-oa-opendata`, inventory of
2026-11-15; 9,343,874 PMCIDs, newest version of each). PMCIDs were shuffled with seed 20261116 and
their metadata read in that order (6,960 read) until every stratum was full.

- **Main corpus (n = 200):** open-access subset, commercial-use licences (199 CC BY, 1 CC0 — the
  former `oa_comm` subset), published journal versions with the publisher PDF in the bucket (no
  preprints, author manuscripts, scanned back issues or retractions); article types
  research/review/brief/case report etc. with ≥ 800 body words. Stratified by year — ≤ 2009,
  2010–14, 2015–18, 2019–21, 2022–26, 40 each — and at most one article per journal (200 journals).
  Publishers: BMC 54, MDPI 26, Wiley 22, Frontiers 12, PLOS 6, Nature Publishing Group 5, OUP 4,
  15 without a publisher name in the XML, and 56 articles from 44 other publishers (≤ 3 each). Article types:
  174 research articles, 17 reviews, 9 other. PDFs: median 11 pages (IQR 8–14), 2,393 pages in total;
  body text median 4,512 words. 51 drawn candidates were excluded (no body or < 800 words 23,
  letters/editorials/corrections/abstracts and other non-research types 20, no paragraphs 2,
  manuscripts without figures 6; list in `results/corpus_excluded.csv`).
- **Author-manuscript subset (n = 30):** NIHMS author manuscripts (`is_manuscript`) with a PDF in
  the bucket and at least one figure, one per journal, 2011–2026, median 21 pages. Only about one in ten
  manuscripts in the bucket carries a PDF.
- PMCIDs, versions, journals, publishers, years and DOIs: `results/corpus.csv`.

## Results by metric

### 1. Text fidelity
Median token F1 0.955 (current Swift), 0.890 (baseline), 0.960 (Rust; 0.952 before its hidden-text fix); 0.949 before the word-spacing
and hidden-text fixes, 0.944 and 0.949 before the figure and front-matter fixes, 0.913 and 0.912
before the reference-list fix. Recall is high (median 0.99: little body text is lost), precision lower
(median 0.94): most errors are *extra* text, not missing text. 20.5 % of the papers are below F1 0.9
(28 % before the word-spacing and hidden-text fixes, 30 % before the figure and front-matter fixes,
47 % before the reference-list fix), almost always because of extra material (see error analysis). Order-free bag-of-words F1 is only 0.02 higher than the LCS F1, so reading-order
errors cost little at the word level. By year, 2015–18 converts best with Swift
(median F1 0.967) and 2010–14 worst (0.943; 2022–26 was worst at 0.936 before the hidden-text fix,
now 0.960); before the reference-list fix 2010–14 was worst (0.851; in 13 of its 40 papers the reference list was not separated from the body, now 0).

### 2. Broken words
Mean 0.8 broken words per 1000 tokens (current) vs 4.2 (baseline) and 0.5 (Rust); medians are
0.11/0.27/0.00. 7 papers still exceed 5 per 1000 with Swift (25 before the word-spacing and
hidden-text fixes, 5 with Rust). Before those fixes it was almost entirely one publisher family: MDPI
PDFs (pdfTeX) averaged 15.0 broken words per 1000 tokens with Swift (20.4 baseline; Rust 0.17), from
the hidden manuscript text of their included figure pages; now 0.14, against 0.92 for all other
publishers (Rust 0.52). Lost compound hyphens ("promoterproximal" for "promoter-proximal") occur at
~0.8 per 1000 tokens in all three converters and did not change.

### 3. Reading order
Paragraph order is right in most papers (median τ = 1.000, 98.6 % of consecutive paragraphs in
order). Interleaving — a paragraph resumed after another paragraph's text intruded — affects 20.5 %
of papers with the current Swift converter (23.5 % before the hidden-text fix, 34.0 % in the baseline;
16.0 % Rust), with a long tail (mean 2.3 events per paper).

### 4. Boilerplate leakage
46 % of papers still have at least one leaked running header/footer, page number or publisher/licence
line in the body text (54 % before the front-matter fix, 63.5 % at baseline; mean 1.02 vs 1.63 leaks
per paper). The fixes removed most publisher footers and first-page licence lines (0.37 per paper,
0.87 at baseline); running headers (0.24 per paper) and page numbers (0.42) remain.

### 5. Figures
Count-based, the converters extract enough figure images (96 % of XML figure count) but also extra
ones (1,128 images for 849 XML figures, 1,223 before the figure fix, baseline 1,308: multi-panel figures still cut
into several images where their pieces are far apart, logos, graphical abstracts; exact figure count in 45 % of papers,
40 % before, baseline 31 %). Only 66 % (Swift) / 72 % (Rust) of the XML figures
are *identified* — an image with a caption that carries the right label — and 57 % of figure captions
are reproduced (≥ 80 % of their first 50 words). The dominant failure is an image without caption
(older BMC PDFs put all figures at the end and list captions in the text). Figures cut from the PDF
for author manuscripts: 96.9 % (see above).

### 6. Tables, equations, references
Tables are kept as images: 68 % of XML tables have a table image (342 for 484; 305 before the table
fix), 66 % identified by label (63 % before the figure fix, 55 % before the table fix; Rust 67 %),
precision 93 %; undetected tables leak as runs of numbers into the text. Display equations
as images: 133 for 277 XML `disp-formula` (current; baseline 218, Rust 131; 198 and 188 on 2026-11-16, before STIX text fonts stopped counting as math) — the count is dominated
by a few equation-heavy papers. Reference lists: median 1.00 of XML references recovered as items
(baseline 0.85), one list item per reference. In 41/200 papers (42 on 2026-11-16) no reference items had been
found (no references heading recognised) and the list stayed in the body text; since the
reference-list fix 3 (two with little usable text, one without references in the XML).

### 7. Highlight survival
See the table above. Moving from the current PDF view to the XML view keeps 89.4 % of highlights on
the right text (73.0 % verbatim; 88.9 % before the symbol-font fix), Rust 90.5 % (90.6 % before it,
89.5 % before its hidden-text fix), author manuscripts 96.5 % (87.2 % for Swift before the
word-spacing and hidden-text fixes; 87.2 %, 89.2 % and 96.0 % before the figure and front-matter
fixes; 84.2 %, 68.4 % and 87.0 % before the reference-list fix). Of the 9.3 % lost (current Swift;
9.8 % in the fifth run, 11.2 % in the third run, 14.3 % before the reference-list fix), a heuristic split (2026-11-16 run): ~18 % are passages that have no counterpart in the XML reading view (table
text, reference entries, boilerplate, extra PDF matter), ~37 % contain garbled or broken words, ~2 %
differ only in citation format ("[12–14]" vs "[12,13,14]"), the rest in run-in captions, merged
columns, symbols or inline math. A cruder re-check on 2026-11-17 (quote's 3-word shingles < 20 % found in the XML view = no counterpart; else any word missing from the XML vocabulary = garbled) gives 16 % / 43 % for this run and 15 % / 44 % for the 2026-11-16 run: the split did not change.
Repeated with `scripts/s10_lost_split.py` (whole text of the XML view) for the second 2026-11-18 run:
18 % / 42 % (the same script gives 18 % / 38 % for the first run); third run: 16 % / 43 %; fourth run: 16 % / 40 %.

### 8. Speed
Swift (PDFKit): median 0.124 s per page (1.17 s per paper; CPU 0.085 s/page; peak RSS 75 MB).
Rust (PDFium): 0.047 s per page (0.42 s per paper; CPU 0.046 s/page; 103 MB). The slowest of the 60
timed papers took 2.1 s with Swift; in the batch run (3 in parallel) the slowest took 5.2 s. On 2026-11-16
the same measurement gave 0.176 and 0.064 s/page and outliers of 7–15 s, but the old and new binaries
take the same time side by side today, so that run was slowed by other work on the machine.

### 9. Swift vs Rust agreement
Median token F1 between the Swift and Rust outputs 0.989 (IQR 0.933–0.998; 0.989 in the third run,
0.988 before the figure and front-matter fixes, 0.990 before the reference-list fix); per-paper
fidelity correlates at r = 0.73 (0.83 in the third run, 0.87 before the figure and front-matter
fixes). Disagreements come from text a page does not show (MDPI's included manuscript pages: Rust
keeps their hidden text as extra words, Swift leaves it out since the fourth run), column detection
on some layouts (JMIR, Frontiers 2014, SAGE) and figure cropping.

## Error analysis (worst cases, inspected by eye)

1. **Extra material in the PDF that is not article body** (largest precision loss). IUCrData
   (PMC10993561, F1 0.19): 10 of 12 pages are appended CIF/structure-factor tables. F1000Research
   (PMC3790607): open peer-review reports and author replies. Undetected tables become number runs
   ("age 46 68 14 38 67 20 16 14 0 01 sex female 94 …", PMC12714069).
2. **Reference list not separated** (41/200 papers in the first 2026-11-18 run; 42 on 2026-11-16; **fixed
   in 38**, see "What the reference-list fix changed"): no stand-alone "References" heading found, e.g.
   BMC 2015 (PMC4448151, precision 0.51, now 0.98): "… Received 24 June 2025 Accepted 11 August 2025
   References 1 Hanahan D. Hallmarks of cancer …" stayed in the last paragraph.
3. **Split words, mostly MDPI pdfTeX PDFs (fixed in the fourth run)**: "partitioni ng",
   "stati onary", "p acking", "ther efore" (PMC10535837: 33.6 broken per 1000 tokens with Swift,
   0.2 with Rust; now 0). Not glyph spacing, as first thought: each figure is a whole manuscript page
   included as a form and cut to the figure by its bounding box, and PDFKit read the manuscript's
   hidden text too, mixed into the article's lines a point away ("In the genome, FACT i s l ocalized
   i n t he g ene bo dy …", PMC10216454). The Rust converter keeps the hidden text as extra words
   (MDPI median F1 0.875, Swift now 0.983). See "What the word-spacing and hidden-text fixes changed".
4. **Column interleaving on specific layouts** (Swift, not Rust): JMIR 2019 (PMC6898888, 91 events):
   "… to introduce the study to potential participants during | approached by parents or YP for advice
   [18]. Both could provide …" — two columns read line by line. Same in Frontiers 2014 (PMC4032908).
   Duplicated headings ("Background Background") in the same PDFs.
5. **Whole text lost**: J Health Popul Nutr 2006 (PMC3013247, non-embedded TrueType fonts): every
   page became a "figure" image, 2 words of text (all three converters); since the figure and
   front-matter fixes its first page is text (recall 0.26), the other pages, framed, are still
   pictures. EHP 2009 (PMC2702416) and a Liebert short report (PMC7347018) lose 50–60 % of the body
   (recall 0.47/0.43 with Swift; 0.30/0.43 before the figure and front-matter fixes, 0.21/0.33 on
   2026-11-16; Rust keeps 0.80 of PMC7347018). Three papers have recall < 0.5 with Swift, two with Rust.
6. **Captions**: images without caption, captions run into the text ("1955–2000Figure 2 The total
   areas of …"), and multi-panel figures cut into several images where their pieces lie more than
   60 pt apart or have running text between them (fewer since the figure fix: 1,223 → 1,128 images).
7. **Residual boilerplate**: page numbers inside sentences, journal footers on older layouts. (The
   "Cells 2023, 12, x FOR PEER REVIEW" heads in 12 MDPI papers were the hidden manuscript text; none
   left with Swift since the fourth run; the Rust views of 11 papers still have them.)

## Limitations

- **Open-access PMC only.** All papers are from PubMed Central's open-access subset (biomedicine and
  life sciences; CC BY/CC0). BMC and MDPI are over-represented (40 %) because they dominate CC BY
  in PMC; physics/CS/maths layouts (arXiv, IEEE, ACM) and closed-access publishers' current templates
  are under-represented. The ≤ 2009 stratum is mostly BMC/PLOS-era open-access layouts.
- **One article per journal** spreads the sample over layouts but under-weights high-volume journals
  relative to what a typical user's library contains.
- **The XML is not a perfect ground truth.** Publishers' XML and PDF can differ (citation ranges
  "[1–3]" vs "[1,2,3]", supplementary or peer-review matter only in the PDF, section titles, back
  matter); some "errors" are legitimate differences. The XML-view control (F1 0.999) shows the
  metric pipeline itself is clean.
- **Heuristic metrics.** Broken-word, boilerplate and interleaving detectors are vocabulary/position
  heuristics (false positives possible, e.g. chemical names; page-number leaks only counted inside
  running text); figure identification needs a caption with a label, so an uncaptioned correct image
  counts as a miss (count-based recall is reported as the upper bound). The lost-highlight breakdown
  is a heuristic split.
- **Simulated highlights** are uniformly random 5–40-word passages from body paragraphs; real
  highlights favour key sentences and may anchor better or worse.
- **Speed** was measured on one Mac also used for other work; the Swift CLI includes process start,
  PDFKit loading and EPUB writing; the per-page cost is not linear (figure-heavy pages dominate).
- The author-manuscript subset is small (n = 30) and limited to manuscripts whose PDF is in the
  AWS bucket.
- **Journal pages**: 30 open-access pages from nature.com, PLOS, Frontiers and seven smaller
  publishers, fetched without a login or JavaScript; 55 of 92 publishers' sites refused the
  automated request, so the dedicated readers for Atypon (Wiley, ACS, SAGE, T&F), Silverchair (OUP),
  Cell Press and ScienceDirect are untested here (they were checked on saved articles and synthetic
  test pages in the unit tests). A page as the browser renders it for a subscriber can differ from
  the open-access page fetched here.
- Only the reading view's text is evaluated, not its visual rendering, math typesetting or the
  converter's online path (XML lookup by DOI), which would replace most PDF conversions in practice.
