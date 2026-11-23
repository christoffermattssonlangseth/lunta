// Reading ReadCube Papers (SmartCite) citations and formatted bibliographies (word/fields.js).
// Run: node --test apple/citations-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const F = require('../citations/word/fields.js');
const X = require('./fixtures-word.js');

const plain = (x) => JSON.parse(JSON.stringify(x));

test('SmartCite tags: the works in order with their DOIs and PubMed ids; the controls by title', () => {
  const tag = X.smartCiteTag([1, 5, 4]);
  assert.deepEqual(plain(F.smartCiteTag(tag)), [
    { id: X.scId(1), doi: '10.1038/nmeth.2563', pmid: '23852452' }, { id: X.scId(5) },
    { id: X.scId(4), doi: '10.1038/s41586-021-03819-2', pmcid: 'PMC8371605' }]);
  assert.deepEqual(plain(F.smartCiteTag(X.smartCiteTag([2], false))), [{ id: X.scId(2) }]);
  assert.equal(F.smartCiteTag('margins:a,b'), null);
  assert.equal(F.smartCiteTag('MENDELEY_BIBLIOGRAPHY'), null);
  assert.equal(F.classifyControl('SmartCite Citation', tag).source, 'ReadCube Papers (SmartCite)');
  assert.equal(F.classifyControl('', tag).kind, 'cite', 'by its tag alone');
  assert.deepEqual(plain(F.classifyControl('SmartCite Citation', 'garbled').items), []);
  assert.deepEqual(plain(F.classifyControl('SmartCite Bibliography', 'Cell Systems+{"language":"en-US"}')), { kind: 'bib', source: 'ReadCube Papers (SmartCite)', style: 'Cell Systems' });
  assert.equal(F.classifyControl('Citation lunta-o:x', 'margins:a'), null);
  assert.equal(F.classifyControl('', ''), null);
});

test('scan: content controls with their title, tag and text (a paragraph a line)', () => {
  const d = X.smartCite([['1–3', [1, 2, 3]]], X.SC_BIB_CELL.slice(0, 2));
  const c = F.scan(d.xml).controls;
  assert.deepEqual(plain(c.map((x) => [x.title, x.text.split('\n').length])), [['SmartCite Citation', 1], ['SmartCite Bibliography', 2]]);
  assert.equal(c[0].text, '1–3');
  assert.equal(F.smartCiteTag(c[0].tag).length, 3);
});

test('numbered citations: ranges, lists, brackets; anything else is not', () => {
  assert.deepEqual(F.citeNumbers('1–4'), [1, 2, 3, 4]);
  assert.deepEqual(F.citeNumbers('6,7'), [6, 7]);
  assert.deepEqual(F.citeNumbers('17–19'), [17, 18, 19]);
  assert.deepEqual(F.citeNumbers('[2, 5-7]'), [2, 5, 6, 7]);
  assert.deepEqual(F.citeNumbers('(3)'), [3]);
  assert.deepEqual(F.citeNumbers('1, 3 – 4'), [1, 3, 4]);
  assert.equal(F.citeNumbers('(Ke et al., 2013)'), null);
  assert.equal(F.citeNumbers('see 4'), null);
});

test('author–year citations: several works, "et al.", "and", a prefix, a year letter, the author in the text', () => {
  assert.deepEqual(plain(F.citeAuthorYears('(Ke et al., 2013; Chen and Zhuang, 2015a)')), [{ family: 'Ke', year: '2013' }, { family: 'Chen', year: '2015a' }]);
  assert.deepEqual(plain(F.citeAuthorYears('Ståhl et al. (2016)')), [{ family: 'Ståhl', year: '2016' }]);
  assert.deepEqual(plain(F.citeAuthorYears('(see Smith & Wu 2019, 2020)')), [{ family: 'Smith', year: '2019' }, { family: 'Smith', year: '2020' }]);
  assert.deepEqual(plain(F.citeAuthorYears('(van der Berg, 2001)')), [{ family: 'van der Berg', year: '2001' }]);
  assert.equal(F.citeAuthorYears('(see above)'), null);
  assert.equal(F.citeAuthorYears('12'), null);
});

test('a reference in Cell, APA, Nature and Vancouver styles', () => {
  const cell = F.referenceMeta(X.SC_BIB_CELL[0].replace(/^1\. /, ''));
  assert.deepEqual(plain(cell), { doi: '10.1038/nmeth.2563', date: '2013', authors: ['R. Ke', 'M. Mignardi', 'A. Pacureanu', 'J. Svedlund', 'J. Botling', 'C. Wählby', 'M. Nilsson'],
    title: 'In situ sequencing for RNA analysis in preserved tissue and cells', journal: 'Nat Methods', volume: '10', pages: '857–860' });
  const apa = F.referenceMeta(X.SC_BIB_APA[1]);
  assert.equal(apa.title, 'Spatially resolved, highly multiplexed RNA profiling in single cells');
  assert.deepEqual(plain([apa.journal, apa.volume, apa.issue, apa.pages, apa.date, apa.authors.length, apa.authors[0]]), ['Science', '348', '6233', 'aaa6090', '2015', 5, 'K. H. Chen']);
  const nature = F.referenceMeta('Jumper, J. et al. Highly accurate protein structure prediction with AlphaFold. Nature 596, 583–589 (2021).');
  assert.deepEqual(plain(nature), { date: '2021', authors: ['J. Jumper'], title: 'Highly accurate protein structure prediction with AlphaFold', journal: 'Nature', volume: '596', pages: '583–589' });
  const nature2 = F.referenceMeta('Chen, K. H., Boettiger, A. N., Moffitt, J. R., Wang, S. & Zhuang, X. Spatially resolved, highly multiplexed RNA profiling in single cells. Science 348, aaa6090 (2015).');
  assert.deepEqual(plain([nature2.authors.length, nature2.authors[4], nature2.title, nature2.journal, nature2.date]),
    [5, 'X. Zhuang', 'Spatially resolved, highly multiplexed RNA profiling in single cells', 'Science', '2015']);
  const van = F.referenceMeta('Ke R, Mignardi M, Pacureanu A, Svedlund J, Botling J, Wählby C, et al. In situ sequencing for RNA analysis in preserved tissue and cells. Nat Methods. 2013;10(9):857-60. doi:10.1038/nmeth.2563. PMID: 23852452');
  assert.deepEqual(plain([van.authors[0], van.authors.length, van.title, van.journal, van.date, van.volume, van.issue, van.pages, van.doi, van.pmid]),
    ['R. Ke', 6, 'In situ sequencing for RNA analysis in preserved tissue and cells', 'Nat Methods', '2013', '10', '9', '857-60', '10.1038/nmeth.2563', '23852452']);
  const van2 = F.referenceMeta('Ke R, Mignardi M, Nilsson M. In situ sequencing for RNA analysis in preserved tissue and cells. Nat Methods. 2013;10(9):857-60.');
  assert.deepEqual(plain([van2.authors, van2.title]), [['R. Ke', 'M. Mignardi', 'M. Nilsson'], 'In situ sequencing for RNA analysis in preserved tissue and cells']);
  // an arXiv id, a PubMed Central id; a title with "E. coli" in it
  const pre = F.referenceMeta('Vaswani, A., Shazeer, N., and Polosukhin, I. (2017). Attention is all you need. Preprint at arXiv, arXiv:1706.03762.');
  assert.deepEqual(plain([pre.arxiv, pre.title]), ['1706.03762', 'Attention is all you need']);
  const coli = F.referenceMeta('Doe, J. (2020). Growth of E. coli in the cold. J Bact 12, 1–9. PMCID: PMC1234567');
  assert.deepEqual(plain([coli.title, coli.pmcid, coli.journal]), ['Growth of E. coli in the cold', 'PMC1234567', 'J Bact']);
});

test('DOIs: a full stop after one goes, its own brackets stay', () => {
  assert.equal(F.cleanDOI('https://doi.org/10.1038/nmeth.2563.'), '10.1038/nmeth.2563');
  assert.equal(F.cleanDOI('doi:10.1016/S0140-6736(20)30183-5.'), '10.1016/S0140-6736(20)30183-5');
  assert.equal(F.cleanDOI('10.1000/abc).'), '10.1000/abc');
  assert.equal(F.cleanDOI('not a doi'), '');
  assert.equal(F.referenceMeta('Lancet team (2020). A study. Lancet 395, 497–506. https://doi.org/10.1016/S0140-6736(20)30183-5.').doi, '10.1016/S0140-6736(20)30183-5');
});

test('bibliographies: numbered (a paragraph broken in two joined), "[n]", all on one line, author–year with a heading', () => {
  const cell = F.parseBibliography(X.SC_BIB_CELL.join('\r'));
  assert.deepEqual(cell.map((e) => e.n), [1, 2, 3, 4, 5, 6]);
  assert.equal(cell[2].meta.doi, '10.1126/science.aaf2403');
  assert.equal(cell[4].meta.doi, undefined);
  assert.equal(cell[4].meta.date, '2014');
  const brackets = F.parseBibliography('[1] Ke R, Nilsson M. In situ sequencing. Nat Methods. 2013;10(9):857-60.\n[2] Jumper J, et al. Highly accurate protein structure prediction with AlphaFold. Nature. 2021;596:583-9.');
  assert.deepEqual(plain(brackets.map((e) => [e.n, e.meta.title, e.meta.date])), [[1, 'In situ sequencing', '2013'], [2, 'Highly accurate protein structure prediction with AlphaFold', '2021']]);
  const one = F.parseBibliography(X.SC_BIB_CELL.filter((l) => /^\d/.test(l)).join(' '));
  assert.deepEqual(one.map((e) => e.n), [1, 2, 3, 4, 5, 6]);
  assert.equal(one[1].meta.doi, '10.1126/science.aaa6090');
  const apa = F.parseBibliography(X.SC_BIB_APA.join('\n'));
  assert.deepEqual(plain(apa.map((e) => [e.n, e.meta.doi])), [[0, '10.1126/science.aaa6090'], [0, '10.1038/nmeth.2563'], [0, '10.1126/science.aaf2403']]);
  assert.deepEqual(F.parseBibliography(''), []);
});

test('a citation’s works: by the DOI in its tag, else its number or author and year; an entry in the wrong place isn’t taken', () => {
  const entries = F.parseBibliography(X.SC_BIB_CELL.join('\n'));
  const items = F.smartCiteTag(X.smartCiteTag([2, 4]));
  const w = F.formattedCitation('2,4', items, entries);
  assert.deepEqual(plain(w.map((x) => [x.meta.doi, x.meta.date, x.entry])), [['10.1126/science.aaa6090', '2015', 2], ['10.1038/s41586-021-03819-2', '2021', 4]]);
  // numbered differently than the tag's DOIs say (the bibliography renumbered): the DOI wins
  const swapped = F.formattedCitation('4,2', items, entries);
  assert.deepEqual(plain(swapped.map((x) => x.entry)), [2, 4]);
  // no identifiers: the numbers; a number with no entry: only what the tag has
  assert.deepEqual(plain(F.formattedCitation('5', F.smartCiteTag(X.smartCiteTag([5])), entries).map((x) => x.meta.date)), ['2014']);
  assert.deepEqual(plain(F.formattedCitation('9', F.smartCiteTag(X.smartCiteTag([1])), entries).map((x) => x.meta)), [{ doi: '10.1038/nmeth.2563', pmid: '23852452', date: '2013',
    authors: entries[0].meta.authors, title: entries[0].meta.title, journal: 'Nat Methods', volume: '10', pages: '857–860' }]);
  assert.deepEqual(plain(F.formattedCitation('9', [{ id: 'x' }], entries).map((x) => x.meta)), [{}]);
  // no tag at all (another add-in's formatted citations): the numbers alone
  assert.deepEqual(plain(F.formattedCitation('1–2', [], entries).map((x) => x.meta.doi)), ['10.1038/nmeth.2563', '10.1126/science.aaa6090']);
  assert.equal(F.formattedCitation('see above', [], entries), null);
  // author–year, two papers of one author in a year told apart by their letter
  const ay = F.parseBibliography('Smith, J. (2020a). First study of cells. Cell 1, 1–2.\nSmith, J. (2020b). Second study of cells. Cell 2, 3–4.');
  assert.deepEqual(plain(F.formattedCitation('(Smith, 2020b; Smith, 2020a)', [], ay).map((x) => x.meta.title)), ['Second study of cells', 'First study of cells']);
  assert.deepEqual(plain(F.formattedCitation('(Smith, 2020)', [], ay).map((x) => x.meta)), [{}], 'ambiguous without the letter');
});
