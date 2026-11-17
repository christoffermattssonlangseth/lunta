// The References panel's page side (apple/Shared/References.swift): luntaReferences() on small
// reading views of each kind the converters make.
// Run: node --test scripts/reader-tests/

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, run } from './mini-dom.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const src = readFileSync(join(repo, 'apple/Shared/References.swift'), 'utf8').replace(/\r\n/g, '\n');
const m = src.match(/static let referencesJS = #"""\n([\s\S]*?)\n([ \t]*)"""#/);
assert.ok(m, 'referencesJS literal');
const script = m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n');

function refs(html) {
  const doc = parse(html);
  let marked = 0;
  const win = run(script, doc, { luntaMarkJump: () => { marked++; } });
  return { list: JSON.parse(win.luntaReferences()), win, doc, marked: () => marked };
}

test('JATS / PMC: ol.refs, DOI and PubMed links, numbers from the list, citations counted', () => {
  const { list, win, doc, marked } = refs(`
    <section><h1 id="s1">Introduction</h1>
      <p>As shown before <a href="#pone.0001-ref1">1</a>, and again <a href="#pone.0001-ref1">1</a>, <a href="#pone.0001-ref2">2</a>.</p>
      <p>A <a href="#fig1">figure</a> and <a href="https://example.org/">a web link</a>.</p>
    </section>
    <section><h1 id="references">References</h1><ol class="refs">
      <li id="pone.0001-ref1">Smith J, Lee K. 2020. A title. <em>J Biol</em> <strong>1</strong>:2–3. <a href="https://doi.org/10.1371/journal.pone.0000001">doi:10.1371/journal.pone.0000001</a></li>
      <li id="pone.0001-ref2">Doe A. 2019. Another. <em>Cell</em>. <a href="https://pubmed.ncbi.nlm.nih.gov/31234567">PMID 31234567</a></li>
      <li id="pone.0001-ref3">World Health Organization. Report on things. Geneva; 2018.</li>
    </ol></section>`);
  assert.equal(list.length, 3);
  assert.deepEqual(list.map((r) => r.label), ['1', '2', '3']);
  assert.equal(list[0].doi, '10.1371/journal.pone.0000001');
  assert.equal(list[0].anchor, 'pone.0001-ref1');
  assert.equal(list[0].cited, 2);
  assert.match(list[0].text, /^Smith J, Lee K\. 2020\. A title\./);
  assert.equal(list[1].doi, '');
  assert.equal(list[1].pmid, '31234567');
  assert.equal(list[1].cited, 1);
  assert.equal(list[2].doi + list[2].arxiv + list[2].pmid + list[2].pmcid, '');
  assert.equal(list[2].cited, 0);
  // jumping: marks the place for Back, scrolls to the entry and flashes it
  assert.equal(win.luntaShowReference(1), true);
  assert.equal(doc.scrolledTo.id, 'pone.0001-ref2');
  assert.equal(marked(), 1);
  assert.equal(win.luntaShowReference(9), false);
});

test('arXiv: labels kept apart, arXiv ids from text and arXiv DOIs, links into the list counted', () => {
  const { list } = refs(`
    <p>Transformers <a href="#bib.bib1">[1]</a> and BERT <a href="#bib.bib2">Devlin et al. (2019)</a>; see also <a href="#bib.bib3">[3]</a>.</p>
    <h2 id="bib.sec">References</h2>
    <ol class="refs" id="bib">
      <li id="bib.bib1"><span class="label">[1]</span> A. Vaswani, N. Shazeer, et al. (2017) Attention is all you need. arXiv preprint arXiv:1706.03762v5.</li>
      <li id="bib.bib2"><span class="label">Devlin et al. (2019)</span> J. Devlin et al. BERT: pre-training. In NAACL, <a href="https://doi.org/10.18653/v1/N19-1423">Link</a>.</li>
      <li id="bib.bib3"><span class="label">[3]</span> K. He et al. Deep residual learning. <a href="https://doi.org/10.48550/arXiv.1512.03385">doi</a>. See <a href="#bib.bib1">[1]</a>.</li>
      <li id="bib.bib4"><span class="label">[4]</span> E. Witten. Old style. hep-th/9802150 is not marked; arXiv:hep-th/9802150.</li>
    </ol>`);
  assert.equal(list.length, 4);
  assert.deepEqual(list.map((r) => r.label), ['1', 'Devlin et al. (2019)', '3', '4']);
  assert.equal(list[0].arxiv, '1706.03762');
  assert.equal(list[0].doi, '');
  assert.match(list[0].text, /^A\. Vaswani/);
  assert.equal(list[1].doi, '10.18653/v1/N19-1423');
  assert.equal(list[2].arxiv, '1512.03385');
  assert.equal(list[3].arxiv, 'hep-th/9802150');
  // the link from entry 3 to entry 1 is not an in-text citation
  assert.deepEqual(list.map((r) => r.cited), [1, 1, 1, 0]);
});

test('from a PDF: paragraphs after the References heading, up to the next section', () => {
  const { list, win, doc } = refs(`
    <div class="section-start" id="section-2"></div><h1 id="p2e-s3">2 Methods</h1><p>We did things [1, 2].</p>
    <div class="section-start" id="section-3"></div><h1 id="p2e-s4">References</h1>
    <p>1. Someone A, Other B. A paper about boxes. Journal of Boxes 12, 3–4 (2020). doi:10.1234/boxes.2020.5678.</p>
    <p>[2] Other B. A preprint. arXiv:2101.00001v2 (2021).</p>
    <p>3 Third C. A book. Publisher, 1999.</p>
    <p>Fourth D (2001) PMID: 11223344. Retrieved from PMC1234567.</p>
    <div class="section-start" id="section-4"></div><h1 id="p2e-s5">Appendix</h1><p>1. Not a reference at all.</p>`);
  assert.equal(list.length, 4);
  assert.deepEqual(list.map((r) => r.label), ['1', '2', '3', '']);
  assert.equal(list[0].doi, '10.1234/boxes.2020.5678');
  assert.match(list[0].text, /^Someone A, Other B\./);
  assert.equal(list[1].arxiv, '2101.00001');
  assert.match(list[1].text, /^Other B\./);
  assert.equal(list[2].text, 'Third C. A book. Publisher, 1999.');
  assert.equal(list[3].pmid, '11223344');
  assert.equal(list[3].pmcid, 'PMC1234567');
  assert.ok(list.every((r) => r.anchor === '' && r.cited === 0));
  // no ids: the jump goes to the paragraph itself
  assert.equal(win.luntaShowReference(2), true);
  assert.match(doc.scrolledTo.textContent, /^3 Third C/);
});

test('journal pages: a marked reference section, and an unmarked list under a heading', () => {
  const marked = refs(`
    <p>Shown by <a href="#r1">Hopper (1952)</a> and <a href="article.html#r2">others</a>.</p>
    <div class="ref-list"><h2>Literature cited</h2><ul>
      <li><a id="r1"></a>Hopper G. The education of a computer. 1952. <a href="https://doi.org/10.1145%2F609784.609818">CrossRef</a></li>
      <li><a id="r2"></a>Knuth D. Literate programming. <a href="https://www.ncbi.nlm.nih.gov/pmc/articles/PMC7654321/">PMC free article</a></li>
    </ul></div>`).list;
  assert.equal(marked.length, 2);
  assert.equal(marked[0].doi, '10.1145/609784.609818');
  assert.equal(marked[0].anchor, 'r1');
  assert.equal(marked[0].label, '');
  assert.equal(marked[0].cited, 1);
  assert.equal(marked[1].pmcid, 'PMC7654321');
  assert.equal(marked[1].cited, 1);

  const plain = refs(`
    <section id="refs"><h2>References</h2><ol start="5"><li id="ref5">Hopper G. The education of a computer. 1952.</li><li>Lovelace A. Notes. 1843.</li></ol></section>
    <section><h2>Acknowledgements</h2><p>Thanks.</p></section>`).list;
  assert.deepEqual(plain.map((r) => [r.label, r.anchor]), [['5', 'ref5'], ['6', '']]);

  // no reference list at all
  assert.deepEqual(refs('<h1>Intro</h1><p>Text.</p><ol><li>A list item that is not a reference.</li></ol>').list, []);
});
