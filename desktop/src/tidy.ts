// Tidy Library…, as in the Apple and Android apps: Lunta proposes, the researcher decides. One
// window lists what could be tidied, each with its own buttons: possible duplicates (Merge, Keep
// Selected after a confirm, or Not Duplicates), missing details (Find DOIs, Find PDFs, Make
// Reading Views, Switch to the Published Version), topics to make into tags or folders (found
// from shared words, or suggested by the on-device AI) and existing tags for untagged papers.
// Nothing changes until a button is pressed; tags, folders and DOIs applied can be undone. The
// work is the core's (src-tauri/src/tidy.rs → core/src/tidy).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ask } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';
import { api, type Paper, type PaperMeta } from './api';
import { h, status, authorsShort } from './ui';
import { switchTo, type PublishedVersion } from './checks';
import './tidy.css';

interface DuplicateGroup { key: string; paper_ids: string[]; reason: string; reason_text: string; keep: string; note: string; }
interface PublishedSuggestion { paper_id: string; version: PublishedVersion; }
interface MissingDetails { no_doi: string[]; no_pdf: string[]; no_reading_view: string[]; published: PublishedSuggestion[]; }
interface GroupProposal { name: string; paper_ids: string[]; why: string; by_model: boolean; }
interface TagSuggestion { paper_id: string; tag: string; because: string; score: number; }
interface TidyReport { duplicates: DuplicateGroup[]; missing: MissingDetails; topics: GroupProposal[]; tag_suggestions: TagSuggestion[]; papers: number; }
interface DoiSuggestion { paper_id: string; doi: string; found: PaperMeta; source: string; duplicate_of: string | null; }
interface DoiReport { suggestions: DoiSuggestion[]; not_found: string[]; errors: string[]; }
/** What applying did, handed back as it came for Undo. */
interface TidyChange { summary: string; [k: string]: unknown; }

let onChange: () => void = () => {};
/** Called after a change (the library list reloads). */
export function setTidyReload(f: () => void) { onChange = f; }

let findingDois = false;
listen<string>('tidy-progress', (e) => { if (findingDois) status(e.payload, 'busy'); });

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** Tidy Library…: the window. */
export async function openTidy() {
  if (document.querySelector('.tidy-modal')) return;
  status('Looking through the library…', 'busy');
  let report: TidyReport;
  try { report = await invoke<TidyReport>('tidy_report'); }
  catch (e) { status(String(e), 'error'); return; }
  const papers = new Map<string, Paper>();
  const loadPapers = async () => {
    const rows = await api.list({ kind: 'all' }, '', 'added').catch(() => []);
    papers.clear();
    rows.forEach((r) => papers.set(r.paper.id, r.paper));
  };
  await loadPapers();
  const modelReady = await invoke<boolean>('tidy_model_ready').catch(() => false);
  status('');

  // what the window holds between refreshes
  const keep = new Map<string, string>();               // duplicate group -> paper to keep
  let dois: DoiReport | null = null;
  const doiTicks = new Set<string>();
  // the topics shown (names editable); a dismissal keys the name as proposed (the core normalizes it)
  const proposed = new WeakMap<GroupProposal, string>();
  const copyTopics = (ts: GroupProposal[]) => ts.map((t) => { const c = { ...t }; proposed.set(c, t.name); return c; });
  let topics: GroupProposal[] = copyTopics(report.topics);
  let topicsByModel = false;
  const topicTicks = new Set<GroupProposal>();
  const tagTicks = new Set<string>();
  const tagKey = (s: TagSuggestion) => `${s.paper_id}:${s.tag}`;
  report.tag_suggestions.forEach((s) => tagTicks.add(tagKey(s)));
  const undo: { dois?: TidyChange; groups?: TidyChange; tags?: TidyChange } = {};
  const open = new Set<string>();                        // <details> left open
  let working = false;
  let stopPdfs = false;
  let findingPdfs = false;

  const list = h('div.check-list.tidy-list');
  const close = () => { stopPdfs = true; back.remove(); };
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) close(); } },
    h('div.modal.wide.check-modal.tidy-modal', {},
      h('h3', { text: 'Tidy Library' }),
      h('p.hint', { text: 'Lunta suggests; nothing changes until you choose. Tags, folders and DOIs you apply can be undone here; merging can’t.' }),
      list,
      h('div.buttons', {},
        h('button', { type: 'button', text: 'Suggest Dismissed Again', title: 'Show the suggestions you dismissed on this computer again', onclick: async () => {
          await invoke('tidy_reset_dismissed').catch((e) => status(String(e), 'error'));
          await refresh(true);
        } }),
        h('button.primary', { type: 'button', text: 'Done', onclick: () => close() }))));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') close(); });
  document.body.append(back);

  /** Reads the report again (and the papers); `libraryChanged` reloads the main list too. */
  async function refresh(libraryChanged: boolean) {
    try { report = await invoke<TidyReport>('tidy_report'); } catch (e) { status(String(e), 'error'); }
    await loadPapers();
    if (!topicsByModel) topics = copyTopics(report.topics);
    else topics = topics.filter((t) => t.paper_ids.some((id) => papers.has(id)));
    topicTicks.forEach((t) => { if (!topics.includes(t)) topicTicks.delete(t); });
    if (dois) dois.suggestions = dois.suggestions.filter((s) => report.missing.no_doi.includes(s.paper_id));
    if (libraryChanged) onChange();
    render();
  }

  /** Runs one change at a time, with a status line. */
  async function act<T>(busy: string, f: () => Promise<T>): Promise<T | undefined> {
    if (working) { status('Wait for the last change to finish.', 'error'); return undefined; }
    working = true;
    status(busy, 'busy');
    try { const r = await f(); status(''); return r; }
    catch (e) { status(String(e), 'error'); return undefined; }
    finally { working = false; }
  }

  const title = (id: string) => papers.get(id)?.meta.title || '(a paper no longer in the library)';
  const link = (text: string, onclick: () => void, tip = '') =>
    h('button.link', { type: 'button', text, title: tip || undefined, onclick: (e: Event) => { e.preventDefault(); onclick(); } });
  const web = (text: string, url: string) => link(text, () => openUrl(url));
  const dismiss = (key: string, label = 'Dismiss') => link(label, async () => {
    await invoke('tidy_dismiss', { key }).catch((e) => status(String(e), 'error'));
    await refresh(false);
  }, 'Don’t suggest this again on this computer');
  const details = (key: string, summary: string, kids: HTMLElement[]) => {
    const d = h('details', { open: open.has(key) }, h('summary', { text: summary }), ...kids) as HTMLDetailsElement;
    d.addEventListener('toggle', () => { if (d.open) open.add(key); else open.delete(key); });
    return d;
  };
  const undoLine = (which: 'dois' | 'groups' | 'tags') => {
    const c = undo[which];
    if (!c) return null;
    return h('div.tidy-undo', {}, h('span', { text: c.summary }), ' ', link('Undo', async () => {
      const ok = await act('Undoing…', () => invoke('tidy_undo', { change: c }));
      if (ok === undefined) return;
      undo[which] = undefined;
      status('Undone.');
      await refresh(true);
    }));
  };

  /** A paper: title, then authors · year · journal and what it has. */
  function paperLine(id: string): HTMLElement {
    const p = papers.get(id);
    if (!p) return h('div.tidy-paper', {}, h('div.tidy-title', { text: title(id) }));
    return h('div.tidy-paper', {},
      h('div.tidy-title', { text: p.meta.title || '(untitled)' }),
      h('div.hint', {}, [authorsShort(p.meta.authors), p.year, p.meta.journal].filter(Boolean).join(' · '), ' ',
        p.has_pdf ? h('span.badge', { text: 'PDF' }) : null,
        p.has_epub ? h('span.badge', { text: 'Reading view' }) : null,
        p.annotation_count ? h('span.badge.hl', { text: `✎${p.annotation_count}`, title: 'Highlights and notes' }) : null,
        p.meta.doi ? h('span.badge', { text: 'DOI', title: p.meta.doi }) : null));
  }

  // ---- possible duplicates ----

  function duplicates(): HTMLElement | null {
    const groups = report.duplicates;
    if (!groups.length) return null;
    return h('div.check-section', {}, h('h4', { text: `Possible duplicates (${groups.length})` }),
      ...groups.map((g) => {
        if (!keep.has(g.key) || !g.paper_ids.includes(keep.get(g.key)!)) keep.set(g.key, g.keep);
        return h('div.tidy-group', {},
          h('div.tidy-reason', { text: g.reason_text }),
          ...g.paper_ids.map((id) => h('label.tidy-pick', {},
            h('input', { type: 'radio', name: `keep-${g.key}`, checked: keep.get(g.key) === id, onchange: () => keep.set(g.key, id) }),
            paperLine(id))),
          g.note ? h('div.hint', { text: g.note }) : null,
          h('div.tidy-actions', {},
            h('button', { type: 'button', text: 'Merge, Keep Selected…', onclick: () => merge(g) }),
            h('button', { type: 'button', text: 'Not Duplicates', onclick: async () => {
              await invoke('tidy_dismiss', { key: g.key }).catch((e) => status(String(e), 'error'));
              await refresh(false);
            } })));
      }));
  }

  async function merge(g: DuplicateGroup) {
    const k = keep.get(g.key) || g.keep;
    const others = g.paper_ids.filter((id) => id !== k);
    const ok = await ask(`Merge ${plural(others.length, 'paper', 'papers')} into “${title(k)}”? Their missing details, tags, folders, highlights and notes, and a PDF or reading view it lacks go to the paper you keep; then ${others.length === 1 ? 'the other is' : 'the others are'} deleted on every device. This can’t be undone.`,
      { title: 'Merge Duplicates', kind: 'warning', okLabel: 'Merge', cancelLabel: 'Cancel' });
    if (!ok) return;
    const r = await act('Merging…', () => invoke('tidy_merge', { keep: k, others }));
    if (r === undefined) return;
    status(`Merged into “${title(k).slice(0, 60)}”.`);
    await refresh(true);
  }

  // ---- missing details ----

  function missing(): HTMLElement | null {
    const m = report.missing;
    if (!m.no_doi.length && !m.no_pdf.length && !m.no_reading_view.length && !m.published.length && !undo.dois) return null;
    const titles = (key: string, ids: string[], prefix: string) => details(key, 'Show the papers',
      ids.map((id) => h('div.tidy-row', {}, h('span.tidy-grow', { text: title(id) }), dismiss(`${prefix}:${id}`))));
    return h('div.check-section', {}, h('h4', { text: 'Missing details' }),
      m.no_doi.length || dois || undo.dois ? h('div.tidy-group', {},
        h('div.tidy-head', {}, h('strong', { text: `No DOI (${m.no_doi.length})` }),
          m.no_doi.length ? h('button', { type: 'button', text: dois ? 'Find DOIs Again' : 'Find DOIs', onclick: findDois, disabled: findingDois }) : null),
        h('div.hint', { text: 'Articles, conference papers, chapters and preprints without one. Find DOIs looks them up (arXiv, then Crossref) and takes a DOI only when the title, year and first author agree; you choose which to add.' }),
        m.no_doi.length ? titles('no-doi', m.no_doi, 'doi') : null,
        doiResults(),
        undoLine('dois')) : null,
      m.no_pdf.length ? h('div.tidy-group', {},
        h('div.tidy-head', {}, h('strong', { text: `No PDF (${m.no_pdf.length})` }),
          h('button', { type: 'button', text: findingPdfs ? 'Stop' : 'Find PDFs', onclick: () => { if (findingPdfs) stopPdfs = true; else findPdfs(m.no_pdf); } })),
        h('div.hint', { text: 'Find PDFs looks for an open-access PDF of each (Unpaywall, PubMed Central, arXiv…).' }),
        titles('no-pdf', m.no_pdf, 'pdf')) : null,
      m.no_reading_view.length ? h('div.tidy-group', {},
        h('div.tidy-head', {}, h('strong', { text: `No reading view (${m.no_reading_view.length})` }),
          h('button', { type: 'button', text: 'Make Reading Views', onclick: () => makeViews(m.no_reading_view) })),
        h('div.hint', { text: 'Papers with a PDF but no reading view. They are made in the background; the status bar shows how far it has got.' }),
        titles('no-epub', m.no_reading_view, 'epub')) : null,
      m.published.length ? h('div.tidy-group', {},
        h('div.tidy-head', {}, h('strong', { text: `Published preprints (${m.published.length})` })),
        h('div.hint', { text: 'Preprints the checks found published, whose published version isn’t in the library.' }),
        ...m.published.map((s) => h('div.tidy-row', {},
          h('div.tidy-grow', {}, h('div', { text: title(s.paper_id) }),
            h('div.hint', { text: `Published in ${s.version.journal || 'a journal'}${s.version.date ? ` (${s.version.date.slice(0, 4)})` : ''} · ${s.version.doi}` })),
          link('Switch to the Published Version…', async () => {
            const p = papers.get(s.paper_id);
            if (!p) return;
            await switchTo(p, s.version);
            await refresh(true);
          }),
          web('Open', `https://doi.org/${s.version.doi}`)))) : null);
  }

  async function findDois() {
    if (findingDois) return;
    findingDois = true;
    render();
    status(`Looking up DOIs for ${plural(report.missing.no_doi.length, 'paper', 'papers')} (about a second each)…`, 'busy');
    try {
      dois = await invoke<DoiReport>('tidy_find_dois', { ids: [] });
      doiTicks.clear();
      dois.suggestions.forEach((s) => { if (!s.duplicate_of) doiTicks.add(s.paper_id); });
      const parts = [`Found ${plural(dois.suggestions.length, 'DOI', 'DOIs')}`];
      if (dois.not_found.length) parts.push(`${dois.not_found.length} not found`);
      if (dois.errors.length) parts.push(`${dois.errors.length} couldn’t be looked up`);
      status(parts.join('; ') + '.', 'info', true);
      open.add('dois');
    } catch (e) { status(String(e), 'error'); }
    finally { findingDois = false; }
    render();
  }

  function doiResults(): HTMLElement | null {
    if (!dois) return null;
    const usable = dois.suggestions.filter((s) => !s.duplicate_of);
    const rows = dois.suggestions.map((s) => {
      const f = s.found;
      return h('label.tidy-row', {},
        h('input', { type: 'checkbox', checked: !s.duplicate_of && doiTicks.has(s.paper_id), disabled: !!s.duplicate_of,
          onchange: (e: Event) => { if ((e.target as HTMLInputElement).checked) doiTicks.add(s.paper_id); else doiTicks.delete(s.paper_id); } }),
        h('div.tidy-grow', {},
          h('div', { text: title(s.paper_id) }),
          h('div.hint', { text: `→ ${f.title || '(no title)'}` }),
          h('div.hint', {}, [f.journal, f.date.slice(0, 4)].filter(Boolean).join(' · '), ' · ', web(s.doi, `https://doi.org/${s.doi}`), ` · from ${s.source}`),
          s.duplicate_of ? h('div.check-red', { text: `Another paper has this DOI (“${title(s.duplicate_of).slice(0, 70)}”): merge them instead.` }) : null));
    });
    const apply = async (all: boolean) => {
      const chosen = usable.filter((s) => all || doiTicks.has(s.paper_id));
      if (!chosen.length) { status('Tick the DOIs to add.', 'error'); return; }
      const c = await act('Adding the DOIs…', () => invoke<TidyChange>('tidy_apply_dois', { suggestions: chosen }));
      if (!c) return;
      undo.dois = c;
      status(c.summary);
      await refresh(true);
    };
    return h('div.tidy-results', {},
      h('div.hint', { text: [`Found ${plural(dois.suggestions.length, 'DOI', 'DOIs')}`, dois.not_found.length ? `${dois.not_found.length} not found` : '',
        dois.errors.length ? `${dois.errors.length} couldn’t be looked up` : ''].filter(Boolean).join(' · ') + '.' }),
      ...rows,
      usable.length ? h('div.tidy-actions', {},
        h('button', { type: 'button', text: 'Apply Selected', onclick: () => apply(false) }),
        h('button', { type: 'button', text: 'Apply All', onclick: () => apply(true) })) : null);
  }

  async function findPdfs(ids: string[]) {
    if (findingPdfs) return;
    findingPdfs = true;
    stopPdfs = false;
    render();
    let found = 0, i = 0;
    for (const id of ids) {
      if (stopPdfs) break;
      status(`Looking for open-access PDFs: ${++i} of ${ids.length} (${found} found)…`, 'busy');
      const r = await api.findPdf(id).catch(() => null);
      if (r) found++;
    }
    findingPdfs = false;
    status(`Found ${plural(found, 'PDF', 'PDFs')} for ${plural(i, 'paper', 'papers')}${i < ids.length ? ' (stopped)' : ''}.`, 'info', true);
    if (document.body.contains(back)) await refresh(true); else onChange();
  }

  async function makeViews(ids: string[]) {
    const started = await invoke<boolean>('make_reading_views', { ids }).catch((e) => { status(String(e), 'error'); return null; });
    if (started === false) status('Reading views are already being made.');
    else if (started) status(`Making ${plural(ids.length, 'reading view', 'reading views')} in the background…`, 'info', true);
  }

  // ---- topics to make into tags or folders ----

  function topicSection(): HTMLElement | null {
    if (!topics.length && !undo.groups && !modelReady) return null;
    const make = async (asFolders: boolean) => {
      const chosen = topics.filter((t) => topicTicks.has(t) && t.name.trim());
      if (!chosen.length) { status('Tick the topics to make.', 'error'); return; }
      const c = await act(asFolders ? 'Making folders…' : 'Tagging…', () => invoke<TidyChange>('tidy_create_groups', { proposals: chosen, asFolders }));
      if (!c) return;
      undo.groups = c;
      topics = topics.filter((t) => !chosen.includes(t));
      chosen.forEach((t) => topicTicks.delete(t));
      status(c.summary);
      await refresh(true);
    };
    const suggestAi = async () => {
      const r = await act('The on-device AI is reading the titles (up to a minute)…', () => invoke<GroupProposal[]>('tidy_topics_ai'));
      if (r === undefined) return;
      if (!r.length) { status('The on-device AI gave no usable topics; the ones shown were found without it.', 'error'); return; }
      topics = copyTopics(r);
      topicsByModel = true;
      topicTicks.clear();
      render();
    };
    const how = topicsByModel
      ? 'Suggested by the on-device AI (Qwen3 4B) from your latest papers’ titles. Check them: it can be wrong.'
      : 'Found without AI, from words and phrases several papers share in their titles, keywords and abstracts, and from journals.';
    return h('div.check-section', {}, h('h4', { text: 'Suggested tags and folders' }),
      h('div.hint', {}, how, ' ',
        modelReady && !topicsByModel ? h('button', { type: 'button', text: 'Suggest with On-Device AI', onclick: suggestAi }) : null,
        topicsByModel ? link('Show the ones found without AI', () => { topicsByModel = false; topics = copyTopics(report.topics); topicTicks.clear(); render(); }) : null,
        !modelReady ? 'With On-Device AI (Settings ▾ › On-Device AI…), the model can suggest them too.' : null),
      !topics.length ? h('div.hint', { text: 'No topics to suggest.' }) : null,
      ...topics.map((t, i) => {
        const name = h('input.tidy-name', { type: 'text', value: t.name, spellcheck: 'false', oninput: (e: Event) => { t.name = (e.target as HTMLInputElement).value; } });
        return h('div.tidy-topic', {},
          h('div.tidy-row', {},
            h('input', { type: 'checkbox', checked: topicTicks.has(t), onchange: (e: Event) => { if ((e.target as HTMLInputElement).checked) topicTicks.add(t); else topicTicks.delete(t); } }),
            name, h('span.hint', { text: `— ${plural(t.paper_ids.length, 'paper', 'papers')}` }),
            h('span.tidy-grow'),
            link('Dismiss', async () => {
              await invoke('tidy_dismiss', { key: `topic:${proposed.get(t) ?? t.name}` }).catch((e) => status(String(e), 'error'));
              topics = topics.filter((x) => x !== t);
              topicTicks.delete(t);
              render();
            }, 'Don’t suggest this again on this computer')),
          t.why ? h('div.hint.tidy-indent', { text: t.why }) : null,
          h('div.tidy-indent', {}, details(`topic-${i}-${t.name}`, 'Show the papers', t.paper_ids.map((id) => h('div.tidy-small', { text: title(id) })))));
      }),
      topics.length ? h('div.tidy-actions', {},
        h('button', { type: 'button', text: 'Create as Tags', onclick: () => make(false) }),
        h('button', { type: 'button', text: 'Create as Folders', onclick: () => make(true) })) : null,
      undoLine('groups'));
  }

  // ---- existing tags for untagged papers ----

  function tagSection(): HTMLElement | null {
    const sug = report.tag_suggestions;
    if (!sug.length && !undo.tags) return null;
    const apply = async (all: boolean) => {
      const chosen = sug.filter((s) => all || tagTicks.has(tagKey(s)));
      if (!chosen.length) { status('Tick the tags to add.', 'error'); return; }
      const c = await act('Tagging…', () => invoke<TidyChange>('tidy_apply_tags', { suggestions: chosen }));
      if (!c) return;
      undo.tags = c;
      status(c.summary);
      await refresh(true);
    };
    return h('div.check-section', {}, h('h4', { text: `Tags for untagged papers (${sug.length})` }),
      sug.length ? h('div.hint', { text: 'Tags you already use, for papers that have none.' }) : null,
      ...sug.map((s) => h('label.tidy-row', {},
        h('input', { type: 'checkbox', checked: tagTicks.has(tagKey(s)), onchange: (e: Event) => { if ((e.target as HTMLInputElement).checked) tagTicks.add(tagKey(s)); else tagTicks.delete(tagKey(s)); } }),
        h('div.tidy-grow', {}, h('div', {}, title(s.paper_id), ' → ', h('strong', { text: `#${s.tag}` })), s.because ? h('div.hint', { text: `because ${s.because}` }) : null),
        dismiss(`tag:${s.paper_id}:${s.tag}`))),
      sug.length ? h('div.tidy-actions', {},
        h('button', { type: 'button', text: 'Apply Selected', onclick: () => apply(false) }),
        h('button', { type: 'button', text: 'Apply All', onclick: () => apply(true) })) : null,
      undoLine('tags'));
  }

  function render() {
    const scroll = list.scrollTop;
    const sections = [duplicates(), missing(), topicSection(), tagSection()].filter(Boolean) as HTMLElement[];
    list.replaceChildren(
      h('p', { text: `${plural(report.papers, 'paper', 'papers')} in the library.` + (sections.length ? '' : ' Nothing to tidy: no duplicates, missing details or tags to suggest.') }),
      ...sections);
    list.scrollTop = scroll;
  }
  render();
}
