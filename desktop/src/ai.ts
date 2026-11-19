// On-device AI: the open model (Qwen3 4B, run by the core's llama.cpp in src-tauri/src/ai.rs) writes
// the reader's Summary card and answers Ask, as Apple's on-device model does in the Mac app
// (apple/Shared/Summary.swift, AssistantPanel.swift). Nothing leaves the computer; the model is a
// one-time download (Settings ▾ › On-Device AI…).
//
// The card itself is the Apple apps' (summary.js, from SummaryCard.swift): the page says what the
// summary is made from (luntaSummarySource) and draws it (luntaSummaryShow / luntaSummaryHide);
// this file fetches or writes it and passes it on. Summaries are kept with the paper
// (summary.json), so one made on the Mac, iPhone, Android or another PC is shown at once.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { h, status } from './ui';
import './ai.css';

interface AiStatus {
  name: string; size: number; size_label: string; installed: boolean; partial: number;
  downloading: boolean; summaries: boolean; unsupported: string | null; folder: string;
}
interface Source { n: number; text: string; paper_id: string; }
interface Turn { role: 'user' | 'assistant'; text: string; }

const aiApi = {
  status: () => invoke<AiStatus>('ai_status'),
  download: () => invoke<void>('ai_download'),
  cancelDownload: () => invoke<void>('ai_cancel_download'),
  deleteModel: () => invoke<void>('ai_delete_model'),
  setSummaries: (on: boolean) => invoke<void>('ai_set_summaries', { on }),
  summary: (paper: string, title: string, source: string, force: boolean) => invoke<void>('ai_summary', { paper, title, source, force }),
  stopSummary: (paper: string) => invoke<void>('ai_stop_summary', { paper }),
  ask: (request: number, paper: string | null, question: string, history: Turn[]) =>
    invoke<{ sources: Source[] }>('ai_ask', { request, paper, question, history }),
  stopAsk: () => invoke<void>('ai_stop_ask'),
};

const gb = (n: number) => `${(n / 1e9).toFixed(1)} GB`;
const NOTE = 'Written on this computer by an open model (Qwen3 4B). It can be wrong: check what matters against the paper.';

// ---- Settings ▾ › On-Device AI… ----

let progress: { done: number; total: number } | null = null;
const downloadWatchers = new Set<() => void>();
listen<{ done: number; total: number; finished: boolean; error: string | null }>('ai-download', (e) => {
  const d = e.payload;
  progress = d.finished ? null : { done: d.done, total: d.total };
  if (d.finished) {
    if (d.error === null) status('On-device AI is ready: the model is downloaded.');
    else if (d.error) status(`The model's download stopped: ${d.error}`, 'error');
  }
  downloadWatchers.forEach((f) => f());
});

export async function aiSettingsDialog() {
  const body = h('div.ai-settings');
  const done = () => { back.remove(); downloadWatchers.delete(paint); };
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) done(); } },
    h('div.modal.wide', {}, h('h3', { text: 'On-Device AI' }), body,
      h('div.buttons', {}, h('button', { text: 'Done', onclick: done }))));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') done(); });
  async function paint() {
    const s = await aiApi.status().catch((e) => { status(String(e), 'error'); return null; });
    if (!s) return;
    const rows: (HTMLElement | null)[] = [
      h('p.hint', { text: `Summaries at the top of the reading view, and Ask: questions about a paper or your library, answered from its text. ` +
        `Runs on this computer with the open model ${s.name} (Apache-2.0, by Qwen); nothing is sent anywhere. Needs about 4 GB of free memory while it works.` }),
    ];
    if (s.unsupported) rows.push(h('p.error', { text: s.unsupported }));
    else if (s.installed) {
      rows.push(h('p', { text: `✓ ${s.name} is downloaded (${s.size_label}).` }),
        h('div.row', {}, h('button.danger', { text: 'Delete Model', onclick: async () => {
          await aiApi.deleteModel().catch((e) => status(String(e), 'error'));
          status('The model was deleted.'); paint();
        } })));
    } else if (s.downloading || progress) {
      const p = progress || { done: s.partial, total: s.size };
      rows.push(h('progress', { max: p.total, value: p.done }),
        h('div.row', {}, h('span.hint', { text: `${gb(p.done)} of ${gb(p.total)}` }), h('div.spacer'),
          h('button', { text: 'Cancel', onclick: () => aiApi.cancelDownload() })));
    } else {
      const label = s.partial > 0 ? `Resume Download (${gb(s.partial)} of ${s.size_label})` : `Download Model (${s.size_label})`;
      rows.push(h('div.row', {}, h('button.primary', { text: label, onclick: async () => {
        progress = { done: s.partial, total: s.size };
        await aiApi.download().catch((e) => { progress = null; status(String(e), 'error'); });
        paint();
      } })), h('p.hint', { text: `Saved in ${s.folder}. The download can be stopped and resumed; it is checked before use.` }));
    }
    const box = h('input', { type: 'checkbox', checked: s.summaries, onchange: async (e: Event) => {
      await aiApi.setSummaries((e.target as HTMLInputElement).checked).catch((x) => status(String(x), 'error'));
    } });
    rows.push(h('label.chk', {}, box, ' Show summaries in the reader'),
      h('p.hint', { text: 'A few sentences on what the paper did and found, kept with the paper so your other devices show it too. Summaries can be wrong.' }));
    body.replaceChildren(...rows.filter((r): r is HTMLElement => !!r));
  }
  downloadWatchers.add(paint);
  document.body.append(back);
  await paint();
}

// ---- the Summary card ----

type Run = <T = unknown>(code: string) => Promise<T | null>;
let current: { paper: string; title: string; abstract: string; run: Run } | null = null;
/** What the card shows for each paper while it is open (drawn again if the page reloads). */
const shown = new Map<string, { state: string; text: string; note: string }>();

listen<{ paper: string; state: string; text: string; note: string }>('ai-summary', (e) => {
  const s = e.payload;
  if (!current || s.paper !== current.paper) return;
  if (s.state === 'none') { shown.delete(s.paper); current.run('window.luntaSummaryHide && luntaSummaryHide()'); return; }
  const card = { state: s.state, text: s.text, note: s.note };
  shown.set(s.paper, card);
  current.run(`window.luntaSummaryShow && luntaSummaryShow(${JSON.stringify(JSON.stringify(card))})`);
});

async function source(run: Run, abstract: string): Promise<string> {
  const json = await run<string>(`window.luntaSummarySource ? luntaSummarySource(1900, ${JSON.stringify(abstract)}) : ''`);
  try { return json ? String(JSON.parse(json).text || '') : ''; } catch { return ''; }
}

/** The reading view loaded (or loaded again): show the paper's summary, or write it. */
export async function summaryPageReady(paper: string, title: string, abstract: string, run: Run) {
  if (current && current.paper !== paper) summaryClosed(current.paper);
  current = { paper, title, abstract, run };
  const s = await aiApi.status().catch(() => null);
  if (!s || !s.summaries) { run('window.luntaSummaryHide && luntaSummaryHide()'); return; }
  const old = shown.get(paper);
  if (old) run(`window.luntaSummaryShow && luntaSummaryShow(${JSON.stringify(JSON.stringify(old))})`);
  const text = await source(run, abstract);
  if (!text || current?.paper !== paper) return;
  aiApi.summary(paper, title, text, false).catch(() => {});
}

/** The card's ⋯ menu: "regenerate" or "hide". */
export async function summaryAction(action: string) {
  if (!current) return;
  const c = current;
  if (action === 'regenerate') {
    const s = await aiApi.status().catch(() => null);
    if (!s?.installed) { status('Download the model to write summaries on this computer (Settings ▾ › On-Device AI…).'); return; }
    const text = await source(c.run, c.abstract);
    if (text) aiApi.summary(c.paper, c.title, text, true).catch(() => {});
  } else if (action === 'hide') {
    await aiApi.setSummaries(false).catch(() => {});
    aiApi.stopSummary(c.paper).catch(() => {});
    shown.delete(c.paper);
    c.run('window.luntaSummaryHide && luntaSummaryHide()');
    status('Summaries are off. Turn them on again in Settings ▾ › On-Device AI….');
  }
}

/** The reader closed: stop writing its summary. */
export function summaryClosed(paper: string) {
  aiApi.stopSummary(paper).catch(() => {});
  shown.delete(paper);
  if (current?.paper === paper) current = null;
}

// ---- Ask ----

interface Msg { role: 'user' | 'assistant' | 'notice'; text: string; sources?: Source[]; }
let panel: HTMLElement | null = null;
let scope: { paper: string | null; title: string } = { paper: null, title: '' };
let msgs: Msg[] = [];
let request = 0;
let busy = false;
let openPaper: (id: string) => void = () => {};

listen<{ request: number; text: string; finished: boolean; error: string | null }>('ai-ask', (e) => {
  const d = e.payload;
  if (d.request !== request) return;
  const last = msgs[msgs.length - 1];
  if (last?.role === 'assistant') last.text = d.text || last.text;
  if (d.finished) {
    busy = false;
    if (d.error) {
      if (last?.role === 'assistant' && !last.text) msgs.pop();
      if (d.error !== 'cancelled') msgs.push({ role: 'notice', text: d.error });
    }
  }
  paintAsk();
});

/** How Ask opens a paper it cites (main.ts reads it). */
export function setAskOpener(f: (id: string) => void) { openPaper = f; }

/** Opens Ask about one paper (in the reader) or about the library (paper null). */
export function openAsk(paper: string | null, title = '') {
  if (scope.paper !== paper) { msgs = []; request++; busy = false; }
  scope = { paper, title };
  if (!panel) {
    panel = h('aside.ask-panel');
    document.body.append(panel);
  }
  panel.hidden = false;
  paintAsk();
  panel.querySelector<HTMLTextAreaElement>('textarea')?.focus();
}

export function closeAsk() {
  if (!panel) return;
  if (busy) aiApi.stopAsk().catch(() => {});
  panel.hidden = true;
}

/** The reader closed: Ask about that paper goes with it. */
export function askPaperClosed(paper: string) {
  if (scope.paper === paper) { closeAsk(); msgs = []; scope = { paper: null, title: '' }; }
}

/** "… [2] …" with the numbers as buttons that show what they cite. */
function answerText(text: string, sources: Source[]): HTMLElement {
  const el = h('div.ask-text');
  const parts = text.replace(/\*\*/g, '').split(/(\[\d+\])/);
  for (const p of parts) {
    const m = p.match(/^\[(\d+)\]$/);
    const s = m ? sources.find((x) => x.n === Number(m[1])) : undefined;
    if (s) el.append(h('button.ask-cite', { text: String(s.n), title: s.text, onclick: () => scope.paper ? null : openPaper(s.paper_id) }));
    else el.append(p);
  }
  return el;
}

function paintAsk() {
  if (!panel) return;
  const input = h('textarea', { rows: 2, placeholder: scope.paper ? 'Ask about this paper' : 'Ask about your library', disabled: busy }) as HTMLTextAreaElement;
  const send = () => { const q = input.value.trim(); if (q && !busy) askQuestion(q); };
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
    if (e.key === 'Escape') closeAsk();
  });
  const list = h('div.ask-list', {}, msgs.length ? msgs.map((m) => {
    if (m.role === 'user') return h('div.ask-q', { text: m.text });
    if (m.role === 'notice') return h('div.ask-notice', { text: m.text });
    const srcs = m.sources || [];
    return h('div.ask-a', {},
      m.text ? answerText(m.text, srcs) : h('div.ask-text.pending', { text: 'Thinking…' }),
      srcs.length ? h('details.ask-sources', {}, h('summary', { text: scope.paper ? `${srcs.length} passages it was given` : `${srcs.length} papers it found` }),
        srcs.map((s) => h('div.ask-source', {}, h('b', { text: `[${s.n}] ` }),
          scope.paper ? s.text : h('a', { href: '#', text: s.text, onclick: (e: Event) => { e.preventDefault(); openPaper(s.paper_id); } })))) : null);
  }) : h('p.hint', { text: scope.paper
    ? 'Ask about this paper: its methods, numbers, findings. Answers quote the passages they come from.'
    : 'Ask about your library: “which papers measure lactate in CSF?”, “what do I have on sleep and memory?”' }));
  panel.replaceChildren(
    h('header', {}, h('b', { text: scope.paper ? 'Ask about this paper' : 'Ask about your library' }), h('div.spacer'),
      msgs.length ? h('button.icon', { text: '↺', title: 'New conversation', onclick: () => { if (busy) aiApi.stopAsk(); msgs = []; busy = false; request++; paintAsk(); } }) : null,
      h('button.icon', { text: '✕', title: 'Close (Esc)', onclick: closeAsk })),
    list,
    h('div.ask-input', {}, input, busy ? h('button', { text: 'Stop', onclick: () => aiApi.stopAsk() }) : h('button.primary', { text: 'Ask', onclick: send })),
    h('p.ask-note', { text: NOTE }));
  list.scrollTop = list.scrollHeight;
  if (!busy) input.focus();
}

async function askQuestion(q: string) {
  const s = await aiApi.status().catch(() => null);
  if (s && !s.installed) {
    msgs.push({ role: 'user', text: q }, { role: 'notice', text: s.unsupported || 'Ask needs the on-device model. Download it in Settings ▾ › On-Device AI… (one time).' });
    paintAsk();
    if (!s.unsupported) aiSettingsDialog();
    return;
  }
  const history: Turn[] = msgs.filter((m) => m.role !== 'notice' && m.text).map((m) => ({ role: m.role as 'user' | 'assistant', text: m.text }));
  msgs.push({ role: 'user', text: q });
  const answer: Msg = { role: 'assistant', text: '' };
  msgs.push(answer);
  busy = true;
  const r = ++request;
  paintAsk();
  try {
    const started = await aiApi.ask(r, scope.paper, q, history);
    answer.sources = started.sources;
  } catch (e) {
    msgs.pop();
    msgs.push({ role: 'notice', text: String(e) });
    busy = false;
  }
  paintAsk();
}

/** The library's toolbar button. */
export function libraryAskButton(): HTMLElement {
  return h('button#ask-library', { text: '✨ Ask', title: 'Ask about your library (on-device AI)', onclick: () => {
    if (panel && !panel.hidden && scope.paper === null) closeAsk(); else openAsk(null);
  } });
}

export const askOpen = () => !!panel && !panel.hidden;
