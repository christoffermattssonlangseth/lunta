// Outline from Highlights…, as in the Apple and Android apps: the highlights and notes in some
// papers (a folder, a tag, the selected papers, the search results) as an outline by paper and
// section, or grouped into themes by the on-device AI. Every point is a highlight word for word
// or the researcher's own note (marked Note), cited author–year with the citation key; the model
// only groups the points and titles the themes. Copy Markdown or Save as .md. The work is the
// core's (src-tauri/src/tidy.rs outline_make → core/src/outline.rs).
//
// The Summary side: the on-device AI condenses the same highlights into a few sentences, each
// citing the highlights it is based on; the core checks every sentence (one citing nothing, a
// highlight that wasn't given, or saying more than the highlights it cites is left out) —
// summary_make → core/src/outline/summary.rs. A citation shows its quote, with Open in Paper.
import { invoke } from '@tauri-apps/api/core';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';
import { h, status, copy } from './ui';
import './tidy.css';

interface Outline { title: string; markdown: string; html: string; themes: unknown[]; items: number; papers: number; by_model: boolean; note: string; }
interface SummarySource { number: number; annotation_id: string; paper_id: string; quote: string; note: string; section: string; label: string; cite_key: string; reference: string; link: string; }
interface SummarySentence { text: string; citations: number[]; }
interface HighlightSummary { title: string; sentences: SummarySentence[]; bullets: boolean; sources: SummarySource[]; markdown: string; html: string; dropped: number; ok: boolean; items: number; papers: number; note: string; }

const SUMMARY_LABEL = 'Written by the on-device model from your highlights. Check it against the quotes below.';

let openPaper: (id: string, highlight?: string) => void = () => {};
/** A lunta://open link in the outline opens the paper (a summary's citation: at the highlight). */
export function setOutlineOpener(f: (id: string, highlight?: string) => void) { openPaper = f; }

const ALLOWED = new Set(['H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'P', 'UL', 'OL', 'LI', 'A', 'EM', 'STRONG', 'I', 'B', 'CODE', 'BLOCKQUOTE', 'BR']);

/** The outline's HTML, with only plain text elements and links kept (no scripts, styles or attributes). */
function safeHtml(html: string, done: () => void): DocumentFragment {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  const copyNode = (n: Node): Node[] => {
    if (n.nodeType === Node.TEXT_NODE) return [document.createTextNode(n.textContent || '')];
    if (n.nodeType !== Node.ELEMENT_NODE) return [];
    const el = n as Element;
    const kids = [...el.childNodes].flatMap(copyNode);
    if (!ALLOWED.has(el.tagName)) return ['SCRIPT', 'STYLE', 'TITLE', 'HEAD'].includes(el.tagName) ? [] : kids;
    const out = document.createElement(el.tagName.toLowerCase());
    if (el.tagName === 'A') {
      const href = el.getAttribute('href') || '';
      const id = href.match(/^lunta:\/\/open\?id=([\w-]+)/)?.[1];
      out.setAttribute('href', '#');
      out.setAttribute('title', id ? 'Open the paper' : href);
      out.addEventListener('click', (e) => {
        e.preventDefault();
        if (id) { done(); openPaper(id); } else if (/^https?:\/\//.test(href)) openUrl(href);
      });
    }
    out.append(...kids);
    return [out];
  };
  const frag = document.createDocumentFragment();
  frag.append(...[...doc.body.childNodes].flatMap(copyNode));
  return frag;
}

/** The dialog for these papers (`what`: “Folder name”, 3 selected papers…). */
export async function outlineDialog(ids: string[], what: string) {
  if (!ids.length) { status('No papers to outline here.', 'error'); return; }
  const modelReady = await invoke<boolean>('tidy_model_ready').catch(() => false);
  const topic = h('input', { type: 'text', placeholder: 'Topic (optional): only the highlights about it', spellcheck: 'false' }) as HTMLInputElement;
  const useModel = h('input', { type: 'checkbox' }) as HTMLInputElement;
  const preview = h('div.outline-preview');
  preview.hidden = true;
  const note = h('p.hint');
  let outline: Outline | null = null;
  let summary: HighlightSummary | null = null;
  let tab: 'outline' | 'summary' = 'outline';
  let making = false;
  const current = () => (tab === 'summary' && summary?.ok ? { markdown: summary.markdown, title: summary.title, what: 'the summary' } : outline ? { markdown: outline.markdown, title: outline.title, what: 'the outline' } : null);
  const copyBtn = h('button', { type: 'button', text: 'Copy Markdown', disabled: true, onclick: () => { const c = current(); if (c) copy(c.markdown, `${c.what} as Markdown`); } }) as HTMLButtonElement;
  const saveBtn = h('button', { type: 'button', text: 'Save as .md…', disabled: true, onclick: async () => {
    const c = current();
    if (!c) return;
    const path = await saveDialog({ defaultPath: `${c.title.replace(/[\\/:*?"<>|]/g, '-')}.md`, filters: [{ name: 'Markdown', extensions: ['md'] }] });
    if (typeof path !== 'string') return;
    invoke('save_report', { path, text: c.markdown }).then(() => status(`Saved ${path.split(/[\\/]/).pop()}.`)).catch((e) => status(String(e), 'error'));
  } }) as HTMLButtonElement;
  const makeBtn = h('button.primary', { type: 'submit', text: 'Make Outline' }) as HTMLButtonElement;

  // the Summary side
  const length = h('select', {}, h('option', { value: 'Short', text: 'Short (3–5 sentences)' }), h('option', { value: 'Longer', text: 'Longer (6–10 sentences)' })) as HTMLSelectElement;
  const style = h('select', {}, h('option', { value: 'Prose', text: 'Prose' }), h('option', { value: 'Bullets', text: 'Bullet points' })) as HTMLSelectElement;
  length.value = localStorage.getItem('summaryLength') || 'Short';
  style.value = localStorage.getItem('summaryStyle') || 'Prose';
  length.onchange = () => localStorage.setItem('summaryLength', length.value);
  style.onchange = () => localStorage.setItem('summaryStyle', style.value);
  const summaryBox = h('div.outline-preview.summary-preview');
  summaryBox.hidden = true;
  const summaryNote = h('p.hint');
  const summaryControls = h('div.summary-controls', {}, h('label', {}, 'Length ', length), h('label', {}, 'Style ', style));
  const summaryIntro = h('p.hint', { text: modelReady
    ? 'The on-device AI condenses these highlights and notes into a few sentences, each citing the highlights it is based on. Sentences that cite nothing, or say more than the highlights they cite, are left out.'
    : 'Summaries need the on-device AI (Qwen3 4B): download it in Settings ▾ › On-Device AI…. The outline works without it.' });
  const outlineSide = [modelReady
    ? h('label.chk', {}, useModel, 'Group into themes with on-device AI (it titles the themes; the points stay your highlights)')
    : h('p.hint', { text: 'With On-Device AI (Settings ▾ › On-Device AI…), the highlights can be grouped into themes.' }), preview, note];
  const summarySide = [summaryIntro, summaryControls, summaryBox, summaryNote];
  const tabBtns = { outline: h('button', { type: 'button', text: 'Outline' }) as HTMLButtonElement, summary: h('button', { type: 'button', text: 'Summary' }) as HTMLButtonElement };
  function setTab(t: 'outline' | 'summary') {
    tab = t;
    tabBtns.outline.classList.toggle('on', t === 'outline');
    tabBtns.summary.classList.toggle('on', t === 'summary');
    outlineSide.forEach((e) => { e.hidden = t !== 'outline' || (e === preview && !outline); });
    summarySide.forEach((e) => { e.hidden = t !== 'summary' || (e === summaryBox && !summary?.ok) || (e === summaryControls && !modelReady); });
    title.textContent = t === 'outline' ? 'Outline from Highlights' : 'Summarize Highlights';
    makeBtn.textContent = t === 'outline' ? (outline ? 'Make Again' : 'Make Outline') : (summary ? 'Regenerate' : 'Summarize');
    makeBtn.hidden = t === 'summary' && !modelReady;
    const c = current();
    copyBtn.disabled = saveBtn.disabled = !c || (t === 'outline' && !outline?.items);
  }
  tabBtns.outline.onclick = () => setTab('outline');
  tabBtns.summary.onclick = () => setTab('summary');

  const close = () => back.remove();
  async function make() {
    if (making) return;
    making = true;
    makeBtn.disabled = true;
    const ai = modelReady && useModel.checked;
    status(ai ? 'The on-device AI is grouping the highlights (up to a minute)…' : 'Making the outline…', 'busy');
    try {
      outline = await invoke<Outline>('outline_make', { ids, topic: topic.value.trim(), useModel: ai });
      status('');
      preview.replaceChildren(safeHtml(outline.html, close));
      const lines = [outline.note];
      if (outline.by_model) lines.push('The themes’ titles and summaries are the on-device AI’s (Qwen3 4B): check them. Every point is your highlight or note, word for word.');
      else if (ai) lines.push('The on-device AI gave no usable themes: by paper and section instead.');
      note.textContent = lines.filter(Boolean).join(' ');
    } catch (e) { status(String(e), 'error'); }
    finally { making = false; makeBtn.disabled = false; setTab(tab); }
  }

  async function summarize() {
    if (making || !modelReady) return;
    making = true;
    makeBtn.disabled = true;
    status('The on-device AI is summarizing the highlights (a minute or more for many)…', 'busy');
    try {
      const s = await invoke<HighlightSummary>('summary_make', { ids, topic: topic.value.trim(), length: length.value, style: style.value });
      status('');
      if (s.ok) {
        summary = s;
        renderSummary(s);
        summaryNote.textContent = s.note;
      } else {
        // nothing held up: the outline instead
        summary = null;
        note.textContent = s.note;
        making = false;
        setTab('outline');
        await make();
        note.textContent = [s.note, note.textContent].filter(Boolean).join(' ');
      }
    } catch (e) { status(String(e), 'error'); }
    finally { making = false; makeBtn.disabled = false; setTab(tab); }
  }

  function renderSummary(s: HighlightSummary) {
    const card = h('div.summary-card');
    card.hidden = true;
    const rows = new Map<number, HTMLElement>();
    const quote = (x: SummarySource) => (x.quote ? `“${x.quote}”${x.note ? ` — Note: ${x.note}` : ''}` : `Note: ${x.note}`);
    const where = (x: SummarySource) => (x.section ? `${x.label} · ${x.section}` : x.label);
    const openBtn = (x: SummarySource) => h('button.link', { type: 'button', text: 'Open in Paper', onclick: () => { close(); openPaper(x.paper_id, x.annotation_id); } });
    const pick = (n: number) => {
      const x = s.sources.find((y) => y.number === n);
      if (!x) return;
      card.replaceChildren(h('div.summary-card-head', {}, h('b', { text: `[${n}] ${where(x)}` }), h('button.link', { type: 'button', text: '×', title: 'Close', onclick: () => { card.hidden = true; } })),
        h('div', { text: quote(x) }), openBtn(x));
      card.hidden = false;
      rows.forEach((r, k) => r.classList.toggle('on', k === n));
      rows.get(n)?.scrollIntoView({ block: 'nearest' });
    };
    const sentence = (x: SummarySentence) => {
      const m = x.text.match(/^(.*?)([.!?])?$/s);
      const body = m?.[1] ?? x.text;
      const end = m?.[2] ?? '.';
      const cites: (Node | string)[] = [];
      x.citations.forEach((n, i) => { if (i) cites.push(', '); cites.push(h('a.cite', { href: '#', text: String(n), onclick: (e: MouseEvent) => { e.preventDefault(); pick(n); } })); });
      return [`${body} [`, ...cites, `]${end}`];
    };
    const text = s.bullets
      ? h('ul', {}, ...s.sentences.map((x) => h('li', {}, ...sentence(x))))
      : h('p', {}, ...s.sentences.flatMap((x, i) => [...(i ? [' '] : []), ...sentence(x)]));
    const list = s.sources.map((x) => {
      const r = h('div.summary-source', {}, h('span.num', { text: `[${x.number}]` }), h('div', {}, h('div', { text: quote(x) }), h('div.hint', {}, where(x), ' ', openBtn(x))));
      rows.set(x.number, r);
      return r;
    });
    summaryBox.replaceChildren(h('h1', { text: s.title }), text, h('p.hint', { text: SUMMARY_LABEL }), card, h('h2', { text: 'Highlights cited' }), ...list);
  }

  const title = h('h3', { text: 'Outline from Highlights' });
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) close(); } },
    h('form.modal.wide.check-modal.outline-modal', { onsubmit: (e: Event) => { e.preventDefault(); if (tab === 'outline') make(); else summarize(); } },
      title,
      h('div.segmented.outline-tabs', {}, tabBtns.outline, tabBtns.summary),
      h('p.hint', { text: `The highlights and notes in ${what}, each cited with a link to the paper and its citation key.` }),
      topic,
      ...outlineSide, ...summarySide,
      h('div.buttons', {}, copyBtn, saveBtn,
        h('button', { type: 'button', text: 'Done', onclick: () => close() }), makeBtn)));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') close(); });
  document.body.append(back);
  setTab('outline');
  topic.focus();
}
