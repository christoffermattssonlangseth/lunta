// New for You: a daily, ranked feed of new papers for your research, as in the Mac app
// (apple/Shared/Watches.swift: WatchInbox, FeedRow, ForYouSettings, NewWatchView; DiscoverDetail).
// The core (core/src/foryou, through src-tauri/src/foryou.rs) does the work and keeps the state in
// the library's foryou.json and watches.json, shared with the Mac, iPhone and Android. Every ten
// minutes the backend is asked whether the day's papers are due (at the morning time, or as soon
// as Lunta runs after it). No system notification here: the sidebar's count and a status line.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';
import { h, fill, $, status, menu, menuBelow, copy } from './ui';
import './foryou.css';

interface Find {
  key: string; title: string; authors: string[]; byline: string; journal: string; date: string;
  published: string | null; doi: string; link: string; abstract_text: string; open_access: boolean;
  cited_by: number; reason: string; source: string; summary: string | null; preview: string;
  boosted: boolean; in_library: string | null; found: number; is_new: boolean;
}
interface DigestPart { text: string; key: string | null; }
interface Feed {
  has_interests: boolean; has_setup: boolean; interests: string; latest: Find[]; earlier: Find[];
  digest: DigestPart[]; digest_share: string; last_run: number | null; next_morning: number;
  new_count: number; running: boolean; stage: string | null; ai: boolean;
  problem: string | null; fetch_failed: boolean;
}
interface HPaper {
  key: string; number: number; title: string; authors: string[]; byline: string; journal: string; date: string;
  published: string | null; doi: string; link: string; abstract_text: string; line: string; in_library: string | null;
}
interface Highlights {
  on: boolean; journals: string[]; cadence: 'weekly' | 'daily'; subtitle: string; papers: HPaper[];
  digest: { text: string; cites: { number: number; key: string }[] }[]; digest_markdown: string; digest_share: string;
  last_run: number | null; next_run: number; problem: string | null;
}
interface JournalChoice { name: string; on: boolean; followed: boolean; }
interface WatchView { id: string; kind: string; value: string; label: string; describe: string; }
interface Coauthor { name: string; author: string; orcid: string; shared: number; following: boolean; }
interface Settings {
  interests: string; per_day: number; morning: number; preprints: 'any' | 'few' | 'none';
  orcid: { orcid: string; name: string; works: number; coauthors: Coauthor[] } | null;
  watches: WatchView[]; ai: boolean; openalex_key: string;
}

const tz = () => -new Date().getTimezoneOffset();
const fy = {
  feed: () => invoke<Feed>('foryou_feed', { tz: tz() }),
  count: () => invoke<number>('foryou_count'),
  settings: () => invoke<Settings>('foryou_settings'),
  save: (interests: string, perDay: number, morning: number, preprints: string) =>
    invoke<void>('foryou_save_settings', { interests, perDay, morning, preprints, tz: tz() }),
  addWatch: (kind: string, value: string) => invoke<void>('foryou_add_watch', { kind, value, tz: tz() }),
  removeWatch: (id: string) => invoke<void>('foryou_remove_watch', { id }),
  follow: (author: string) => invoke<void>('foryou_follow', { author, tz: tz() }),
  connectOrcid: (orcid: string) => invoke<void>('foryou_connect_orcid', { orcid }),
  disconnectOrcid: () => invoke<void>('foryou_disconnect_orcid'),
  dismiss: (key: string) => invoke<void>('foryou_dismiss', { key }),
  boost: (key: string) => invoke<boolean>('foryou_boost', { key }),
  opened: () => invoke<void>('foryou_mark_opened'),
  add: (key: string, mustRead: boolean) => invoke<string>('foryou_add', { key, mustRead }),
  draft: () => invoke<string | null>('foryou_draft'),
  run: () => invoke<void>('foryou_run', { tz: tz() }),
  setKey: (key: string) => invoke<void>('foryou_set_openalex_key', { key }),
  tick: () => invoke<[string, string] | null>('foryou_tick', { tz: tz() }),
  highlights: () => invoke<Highlights>('foryou_highlights', { tz: tz() }),
  choices: () => invoke<JournalChoice[]>('foryou_highlight_choices'),
  setJournal: (name: string, on: boolean) => invoke<void>('foryou_set_highlight_journal', { name, on, tz: tz() }),
  defaults: () => invoke<void>('foryou_highlights_defaults', { tz: tz() }),
  cadence: (cadence: string) => invoke<void>('foryou_set_highlight_cadence', { cadence, tz: tz() }),
  runHighlights: () => invoke<void>('foryou_highlights_run', { tz: tz() }),
  hDismiss: (key: string) => invoke<void>('foryou_highlight_dismiss', { key }),
  hAdd: (key: string, mustRead: boolean) => invoke<string>('foryou_highlight_add', { key, mustRead }),
};

// ---- the app around it (set by main.ts) ----

let showing = false;                  // the New for You place is selected
let feed: Feed | null = null;
let hl: Highlights | null = null;
let showAllHighlights = false;
let selected: string | null = null;
let showEarlier = false;
let stage: string | null = null;
let running = false;
let badge = 0;
let adding = new Set<string>();
let onBadge: () => void = () => {};
let onOpenPaper: (id: string) => void = () => {};
let onLibraryChanged: () => void = () => {};

/** The sidebar's count: papers found since the feed was last opened here, not added. */
export function forYouCount(): number { return badge; }

/** main.ts: redraw the sidebar when the count changes; show a library paper; reload the library. */
export function setForYouHooks(h: { badge: () => void; openPaper: (id: string) => void; libraryChanged: () => void }) {
  onBadge = h.badge; onOpenPaper = h.openPaper; onLibraryChanged = h.libraryChanged;
}

async function refreshCount() {
  const n = await fy.count().catch(() => 0);
  if (n !== badge) { badge = n; onBadge(); }
}

/** At start and every ten minutes: the day's run when it's due; a line when a new feed is in. */
export function startForYou() {
  const tick = async () => {
    const notice = await fy.tick().catch(() => null);
    if (notice && !showing) status(`${notice[0]}: ${notice[1]}`, 'info', true);
    refreshCount();
  };
  tick();
  setInterval(tick, 10 * 60 * 1000);
  // the feed changes on other devices too (it syncs): the count follows the files
  setInterval(() => { if (showing) reloadFeed(); else refreshCount(); }, 60 * 1000);
}

listen<{ running: boolean; stage: string | null; finished: boolean; found: number; error: string | null }>('foryou', (e) => {
  const d = e.payload;
  running = d.running;
  stage = d.stage;
  if (d.finished && d.error) status(`New for You: ${d.error}`, 'error');
  if (d.finished) { refreshCount(); if (showing) reloadFeed(); else if (d.found) status(`New for You · ${d.found} new paper${d.found === 1 ? '' : 's'}`); }
  else if (showing) paintStatus();
});

// ---- the place ----

/** Shows the feed in the list and detail panes (the sidebar's "New for You"). */
export async function showForYou() {
  showing = true;
  await reloadFeed();
  fy.opened().then(refreshCount).catch(() => {});
}

/** Another place was chosen. */
export function hideForYou() {
  if (showing) fy.opened().then(refreshCount).catch(() => {});
  showing = false;
}

export function forYouShowing() { return showing; }

async function reloadFeed() {
  const f = await fy.feed().catch((e) => { status(String(e), 'error'); return null; });
  hl = await fy.highlights().catch(() => null);
  if (!f || !showing) return;
  feed = f;
  // a run that is still loading the model isn't running in the core yet: keep the event's word
  running = f.running || (running && stage !== null);
  stage = f.stage ?? (running ? stage : null);
  const all = [...f.latest, ...f.earlier];
  // the detail pane always shows a paper: the first one, or the next after one is turned down
  const inHighlights = selected?.startsWith('hl:') && hl?.papers.some((p) => 'hl:' + p.key === selected);
  if (!selected || (!inHighlights && !all.some((x) => x.key === selected))) selected = f.latest[0]?.key ?? (showEarlier ? f.earlier[0]?.key : null) ?? null;
  paintList();
  paintDetail();
}

const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? '' : 's'}`;
const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

function ago(ms: number): string {
  const min = Math.round((Date.now() - ms) / 60000);
  if (min < 1) return 'just now';
  if (min < 60) return `${min} min ago`;
  const hours = Math.round(min / 60);
  if (hours < 24) return plural(hours, 'hour') + ' ago';
  const days = Math.round(hours / 24);
  return days === 1 ? 'yesterday' : `${days} days ago`;
}

function paintStatus() {
  const el = document.querySelector('#list .fy-status');
  if (el && feed) el.replaceWith(statusBar(feed));
}

function statusBar(f: Feed): HTMLElement {
  const line = running ? (stage || 'Looking for new papers…')
    : f.last_run ? `Picked ${ago(f.last_run)} · next ${time(f.next_morning)}` : 'Getting your first papers…';
  return h('div.fy-status', {},
    h('div.fy-status-text', {}, h('div', { text: line }), h('div.fy-dim', { text: f.interests })),
    running ? h('span.fy-spin', { title: 'Working…' })
      : h('button.icon', { text: '↻', title: 'Look for new papers now', onclick: () => fy.run().catch((e) => status(String(e), 'error')).then(() => { running = true; stage = 'Looking for new papers…'; paintStatus(); }) }),
    h('button.icon', { text: '⚙', title: 'Your interests and what you follow', onclick: settingsDialog }));
}

function paintList() {
  const f = feed;
  if (!f) return;
  const list = $('#list');
  const kids: (HTMLElement | null)[] = [];
  if (!f.has_interests) {
    kids.push(h('div.fy-intro', {},
      h('h3', { text: '✦ Papers picked for your research' }),
      h('p', { text: 'Describe what you work on in a few sentences. Every day Lunta searches OpenAlex, bioRxiv and medRxiv, ranks what’s new against your interests and your library, and, with the on-device model, reads the best abstracts and says why each one fits you.' }),
      h('button.primary', { text: 'Describe Your Research…', onclick: settingsDialog })));
    if (f.has_setup) kids.push(statusBar(f));
  } else {
    kids.push(statusBar(f));
  }
  if (f.problem && !running) kids.push(problemBox(f));
  if (f.has_interests && f.latest.length && f.digest.length) kids.push(digestBox(f));
  if (f.latest.length) kids.push(...f.latest.map(row));
  else if (f.has_interests && !running && !f.problem) {
    kids.push(h('p.fy-empty', { text: (f.earlier.length ? 'Nothing new today.' : 'Nothing yet.') + ' New papers are picked once a day; ↻ looks now.' }));
  }
  if (f.has_setup) kids.push(...highlightsSection());
  if (f.earlier.length) {
    kids.push(h('button.fy-earlier', { onclick: () => { showEarlier = !showEarlier; paintList(); } },
      `${showEarlier ? '▾' : '▸'} Earlier (${f.earlier.length})`));
    if (showEarlier) kids.push(...f.earlier.map(row));
  }
  fill(list, ...kids);
}

/** Fetching failed (or partly): said, with Retry, instead of "Nothing new today". */
function problemBox(f: Feed): HTMLElement {
  const retry = () => fy.run().catch((e) => status(String(e), 'error')).then(() => { running = true; stage = 'Looking for new papers…'; paintList(); });
  return h('div.fy-problem', {},
    h('div.fy-problem-head', { text: f.fetch_failed ? '⚠ Couldn’t reach OpenAlex' : '⚠ Some sources couldn’t be reached' }),
    h('p', { text: f.problem! }),
    h('div.row', {}, h('button', { text: '↻ Retry', onclick: retry }),
      f.problem!.includes('API key') ? h('button', { text: 'Add an API Key…', onclick: settingsDialog }) : null));
}

function digestBox(f: Feed): HTMLElement {
  const text = h('p.fy-digest-text');
  for (const part of f.digest) {
    if (part.key) {
      const key = part.key;
      text.append(h('a', { href: '#', text: part.text, onclick: (e: Event) => { e.preventDefault(); select(key); } }));
    } else text.append(part.text);
  }
  return h('div.fy-digest', {},
    h('div.fy-digest-head', {}, h('span', { text: 'IN SHORT' }), h('div.spacer'),
      h('button.icon', { text: '⧉', title: 'Copy the summary and its papers', onclick: () => copy(f.digest_share, 'summary') })),
    text);
}

// ---- "From the journals you follow" (journal highlights) ----

function highlightsSection(): HTMLElement[] {
  const j = hl;
  if (!j) return [];
  if (!j.on) {
    return [h('div.fy-hl-intro', {},
      h('div.fy-hl-head', { text: 'FROM THE JOURNALS YOU FOLLOW' }),
      h('p', { text: 'Every Monday, the week’s research papers in journals you choose, whatever their topic, with a few sentences on the notable ones.' }),
      h('div.row', {},
        h('button.primary', { text: 'Nature, Science and Cell', onclick: async () => {
          status('Looking for the week’s papers…', 'busy');
          try { await fy.defaults(); } catch (e) { status(String(e), 'error'); }
          reloadFeed();
        } }),
        h('button', { text: 'Choose Journals…', onclick: settingsDialog })))];
  }
  const out: HTMLElement[] = [h('div.fy-hl-title', {},
    h('div', {}, h('div.fy-hl-head', { text: 'FROM THE JOURNALS YOU FOLLOW' }), h('div.fy-dim', { text: j.subtitle || j.journals.join(', ') })),
    h('div.spacer'),
    h('button.icon', { text: '↻', title: 'Look for the latest papers now', onclick: () => fy.runHighlights().then(() => status('Looking for the latest papers in your journals…', 'busy')).catch((e) => status(String(e), 'error')) }))];
  if (j.problem) {
    out.push(h('div.fy-problem', {}, h('div.fy-problem-head', { text: '⚠ Couldn’t get the journals’ papers' }), h('p', { text: j.problem }),
      h('div.row', {}, h('button', { text: '↻ Retry', onclick: () => fy.runHighlights().catch((e) => status(String(e), 'error')) }))));
  }
  if (j.digest.length) {
    const text = h('p.fy-digest-text');
    for (const d of j.digest) {
      text.append(d.text.replace(/[.\s]+$/, '') + ' (');
      d.cites.forEach((c, i) => {
        if (i) text.append(', ');
        text.append(h('a', { href: '#', text: String(c.number), title: j.papers.find((p) => p.key === c.key)?.title ?? '', onclick: (e: Event) => { e.preventDefault(); select('hl:' + c.key); } }));
      });
      text.append('). ');
    }
    out.push(h('div.fy-digest', {},
      h('div.fy-digest-head', {}, h('span', { text: j.cadence === 'daily' ? 'SINCE YESTERDAY' : 'THIS WEEK' }), h('div.spacer'),
        h('button.icon', { text: '⧉', title: 'Copy the summary and its papers', onclick: () => copy(j.digest_share, 'summary') })),
      text));
  }
  if (!j.papers.length && !j.problem) {
    out.push(h('p.fy-empty', { text: j.last_run ? 'No research papers in these journals since the last look.' : 'Looking for the latest papers…' }));
  }
  const shown = showAllHighlights ? j.papers : j.papers.slice(0, 10);
  out.push(...shown.map(hRow));
  if (j.papers.length > 10) {
    out.push(h('button.fy-earlier', { onclick: () => { showAllHighlights = !showAllHighlights; paintList(); } },
      showAllHighlights ? '▾ Fewer' : `▸ All ${j.papers.length} papers`));
  }
  return out;
}

function hRow(p: HPaper): HTMLElement {
  const id = 'hl:' + p.key;
  return h('div.fy-row' + (id === selected ? '.on' : ''), { 'data-key': id, onclick: () => select(id) },
    h('div.fy-title', { text: `${p.number}. ${p.title}` }),
    h('div.fy-byline', { text: p.byline }),
    p.line ? h('div.fy-abstract', { text: p.line }) : null,
    h('div.fy-actions', { onclick: (e: Event) => e.stopPropagation() },
      p.in_library ? h('button', { text: 'Open', onclick: () => onOpenPaper(p.in_library!) })
        : h('button.primary', { text: adding.has(id) ? 'Adding…' : '＋ Add', disabled: adding.has(id), onclick: () => hAdd(p, false) }),
      h('div.spacer'),
      h('button.icon', { text: '👎', title: 'Not for me: hides it', onclick: () => hDismiss(p) })));
}

function hDetail(p: HPaper): HTMLElement {
  const busy = adding.has('hl:' + p.key);
  return h('div.detail.fy-detail', {},
    h('div.fy-beyond', { text: `📰 From ${p.journal}, a journal you follow: a new research paper, whatever its topic.` }),
    h('div.actions', {},
      p.in_library ? h('button.primary', { text: 'Show in Library', onclick: () => onOpenPaper(p.in_library!) })
        : h('button.primary', { text: busy ? 'Adding…' : 'Add to Library', disabled: busy, onclick: () => hAdd(p, false) }),
      p.in_library ? null : h('button', { text: '🔖 Add as Must Read', disabled: busy, onclick: () => hAdd(p, true) }),
      h('div.spacer'),
      h('button.icon', { text: '👎', title: 'Not for Me', onclick: () => hDismiss(p) })),
    h('h2', { text: p.title }),
    h('p.authors', { text: p.authors.join(', ') }),
    h('p.venue', { text: [p.journal, p.date].filter(Boolean).join(' · ') }),
    p.link ? h('p.ids', {}, h('a', { href: '#', text: p.doi ? `DOI ${p.doi}` : p.link, onclick: (e: Event) => { e.preventDefault(); openUrl(p.link); } })) : null,
    p.abstract_text ? h('h4', { text: 'Abstract' }) : null,
    p.abstract_text ? h('p.abstract', { text: p.abstract_text }) : h('p.hint', { text: 'No abstract yet.' }));
}

async function hAdd(p: HPaper, mustRead: boolean) {
  const id = 'hl:' + p.key;
  if (adding.has(id)) return;
  adding.add(id);
  paintList(); paintDetail();
  status(`Adding “${p.title}”…`, 'busy');
  try {
    await fy.hAdd(p.key, mustRead);
    status(mustRead ? `Added “${p.title}” to Must Read` : `Added “${p.title}”`);
    onLibraryChanged();
  } catch (e) { status(String(e), 'error'); }
  adding.delete(id);
  await reloadFeed();
}

async function hDismiss(p: HPaper) {
  try { await fy.hDismiss(p.key); status('Not for you: hidden'); } catch (e) { status(String(e), 'error'); }
  if (selected === 'hl:' + p.key) {
    const i = hl?.papers.findIndex((x) => x.key === p.key) ?? -1;
    const next = hl?.papers[i + 1] ?? hl?.papers[i - 1];
    selected = next ? 'hl:' + next.key : null;
  }
  await reloadFeed();
}

function select(key: string) {
  selected = key;
  if (feed?.earlier.some((x) => x.key === key)) showEarlier = true;
  paintList();
  paintDetail();
  document.querySelector(`#list .fy-row[data-key="${CSS.escape(key)}"]`)?.scrollIntoView({ block: 'nearest' });
}

function row(f: Find): HTMLElement {
  return h('div.fy-row' + (f.key === selected ? '.on' : ''), {
    'data-key': f.key,
    onclick: () => select(f.key),
    oncontextmenu: (e: MouseEvent) => {
      e.preventDefault();
      select(f.key);
      menu(e.clientX, e.clientY, [
        ...(f.in_library ? [{ label: 'Show in Library', action: () => onOpenPaper(f.in_library!) }]
          : [{ label: 'Add to Library', action: () => add(f, false) }, { label: 'Add as Must Read', action: () => add(f, true) }]),
        '-',
        { label: f.boosted ? 'Undo More Like This' : 'More Like This', action: () => boost(f) },
        { label: 'Not for Me', action: () => dismiss(f) },
        '-',
        { label: 'Copy Link', action: () => copy(f.link, 'link'), disabled: !f.link },
      ]);
    },
  },
    h('div.fy-title', { text: f.title }),
    h('div.fy-byline', {}, f.open_access ? h('span.fy-oa', { text: '🔓', title: 'Open access' }) : null, f.byline),
    f.published ? h('div.fy-dim', { text: f.published }) : null,
    f.preview ? h(f.summary ? 'div.fy-summary' : 'div.fy-abstract', { text: f.preview }) : null,
    h('div.fy-reason', { text: `✦ ${f.reason}` }),
    h('div.fy-actions', { onclick: (e: Event) => e.stopPropagation() },
      f.in_library ? h('button', { text: 'Open', title: 'Show it in the library', onclick: () => onOpenPaper(f.in_library!) })
        : addButton(f),
      h('div.spacer'),
      h('button.icon' + (f.boosted ? '.fy-on' : ''), { text: '👍', title: f.boosted ? 'Undo More Like This' : 'More like this: papers like it rank higher', onclick: () => boost(f) }),
      h('button.icon', { text: '👎', title: 'Not for me: hides it, and papers like it rank lower', onclick: () => dismiss(f) })));
}

function addButton(f: Find): HTMLElement {
  const busy = adding.has(f.key);
  const b: HTMLElement = h('button.primary', {
    text: busy ? 'Adding…' : '＋ Add', disabled: busy, title: 'Add to the library (right-click: Add as Must Read)',
    onclick: () => add(f, false),
    oncontextmenu: (e: MouseEvent) => { e.preventDefault(); e.stopPropagation(); menuBelow(b, [{ label: 'Add as Must Read', action: () => add(f, true) }]); },
  });
  return b;
}

function paintDetail() {
  const box = $('#detail');
  const hp = selected?.startsWith('hl:') ? hl?.papers.find((p) => 'hl:' + p.key === selected) : undefined;
  if (hp) { box.replaceChildren(hDetail(hp)); return; }
  const f = feed && [...feed.latest, ...feed.earlier].find((x) => x.key === selected);
  if (!f) {
    box.replaceChildren(h('div.empty', {}, h('p', { text: feed && (feed.latest.length || feed.earlier.length) ? 'Select a paper' : '' })));
    return;
  }
  const busy = adding.has(f.key);
  box.replaceChildren(h('div.detail.fy-detail', {},
    h('div.fy-beyond', { text: '🌍 Beyond your library: a new paper picked for you. Add it to Lunta to read it.' }),
    h('div.actions', {},
      f.in_library ? h('button.primary', { text: 'Show in Library', onclick: () => onOpenPaper(f.in_library!) })
        : h('button.primary', { text: busy ? 'Adding…' : 'Add to Library', disabled: busy, onclick: () => add(f, false) }),
      f.in_library ? null : h('button', { text: '🔖 Add as Must Read', disabled: busy, onclick: () => add(f, true) }),
      h('div.spacer'),
      h('button.icon' + (f.boosted ? '.fy-on' : ''), { text: '👍', title: f.boosted ? 'Undo More Like This' : 'More Like This', onclick: () => boost(f) }),
      h('button.icon', { text: '👎', title: 'Not for Me', onclick: () => dismiss(f) })),
    h('h2', { text: f.title }),
    h('p.authors', { text: f.authors.join(', ') }),
    h('p.venue', { text: [f.journal, f.date, f.cited_by ? `cited by ${f.cited_by}` : '', f.open_access ? 'open access' : ''].filter(Boolean).join(' · ') }),
    f.link ? h('p.ids', {}, h('a', { href: '#', text: f.doi ? `DOI ${f.doi}` : f.link, onclick: (e: Event) => { e.preventDefault(); openUrl(f.link); } })) : null,
    h('h4', { text: 'Why it’s here' }),
    h('p.fy-reason', { text: `✦ ${f.reason}` }),
    f.summary ? h('h4', { text: 'In one sentence' }) : null,
    f.summary ? h('p', { text: f.summary }) : null,
    f.abstract_text ? h('h4', { text: 'Abstract' }) : null,
    f.abstract_text ? h('p.abstract', { text: f.abstract_text }) : h('p.hint', { text: 'No abstract.' }),
  ));
}

async function add(f: Find, mustRead: boolean) {
  if (adding.has(f.key)) return;
  adding.add(f.key);
  paintList(); paintDetail();
  status(`Adding “${f.title}”…`, 'busy');
  try {
    await fy.add(f.key, mustRead);
    status(mustRead ? `Added “${f.title}” to Must Read` : `Added “${f.title}”`);
    onLibraryChanged();
  } catch (e) { status(String(e), 'error'); }
  adding.delete(f.key);
  await reloadFeed();
}

async function boost(f: Find) {
  try { const on = await fy.boost(f.key); status(on ? 'More like this: papers like it rank higher' : 'Undone'); }
  catch (e) { status(String(e), 'error'); }
  reloadFeed();
}

async function dismiss(f: Find) {
  try { await fy.dismiss(f.key); status('Not for you: hidden, and papers like it rank lower'); }
  catch (e) { status(String(e), 'error'); }
  if (selected === f.key) {
    const all = [...(feed?.latest || []), ...(showEarlier ? feed?.earlier || [] : [])];
    const i = all.findIndex((x) => x.key === f.key);
    selected = all[i + 1]?.key ?? all[i - 1]?.key ?? null;
  }
  await reloadFeed();
  refreshCount();
}

// ---- settings (ForYouSettings) ----

const PREPRINTS: [string, string][] = [['any', 'Journals and preprints'], ['few', 'Mostly journals'], ['none', 'Journals only']];
const KINDS: [string, string, string][] = [
  ['journal', 'Journal', 'Journal, e.g. Nature Neuroscience'],
  ['author', 'Author', 'Author’s name or ORCID iD'],
  ['citing', 'Citing…', 'DOI of the paper'],
  ['topic', 'Topic', 'Words, e.g. velocity-based training'],
];

export async function settingsDialog() {
  let s: Settings;
  try { s = await fy.settings(); } catch (e) { status(String(e), 'error'); return; }
  const text = h('textarea.fy-interests', { rows: 7, placeholder: 'A few sentences on your topics, methods and systems.' }) as HTMLTextAreaElement;
  text.value = s.interests;
  const perDay = h('input', { type: 'number', min: 3, max: 20, value: s.per_day }) as HTMLInputElement;
  const morning = h('input', { type: 'time', value: `${String(Math.floor(s.morning / 60)).padStart(2, '0')}:${String(s.morning % 60).padStart(2, '0')}` }) as HTMLInputElement;
  const preprints = h('select', {}, PREPRINTS.map(([v, l]) => h('option', { value: v, text: l, selected: v === s.preprints }))) as HTMLSelectElement;
  const orcidBox = h('div');
  const key = h('input', { type: 'text', spellcheck: 'false', placeholder: 'API key (optional)', value: s.openalex_key }) as HTMLInputElement;
  const watchBox = h('div');
  const hlBox = h('div');
  let changedFollows = false;

  const draftBtn = h('button', {
    text: s.orcid ? '🪄 Draft from My Papers and Library' : '🪄 Draft from My Library',
    title: s.ai ? 'The on-device model writes a paragraph from your papers' : 'Needs the on-device model (Settings ▾ › On-Device AI…)',
    disabled: !s.ai,
    onclick: async () => {
      draftBtn.textContent = s.orcid ? 'Reading your papers…' : 'Reading your library…';
      (draftBtn as HTMLButtonElement).disabled = true;
      try { const d = await fy.draft(); if (d) text.value = d; } catch (e) { status(String(e), 'error'); }
      draftBtn.textContent = s.orcid ? '🪄 Draft from My Papers and Library' : '🪄 Draft from My Library';
      (draftBtn as HTMLButtonElement).disabled = false;
    },
  });

  async function reloadSettings() {
    try { s = { ...(await fy.settings()), interests: text.value }; } catch { /* keep */ }
    paintOrcid(); paintWatches();
  }

  function paintOrcid() {
    if (s.orcid) {
      const o = s.orcid;
      fill(orcidBox,
        h('div.row', {}, h('span', { text: `✓ Connected: ${o.name}, ${plural(o.works, 'paper')}` }), h('div.spacer'),
          h('button.danger', { text: 'Disconnect', onclick: async () => { await fy.disconnectOrcid().catch((e) => status(String(e), 'error')); reloadSettings(); } })),
        h('p.hint', { text: 'Papers citing yours always show up. Your papers rank like your library and are never offered as new.' }),
        o.coauthors.length ? h('h4', { text: 'People you publish with' }) : null,
        ...o.coauthors.map((c) => h('div.row.fy-person', {},
          h('div', {}, h('div', { text: c.name }), h('div.fy-dim', { text: `${plural(c.shared, 'paper')} together` })),
          h('div.spacer'),
          c.following ? h('span.fy-dim', { text: 'Following' })
            : h('button', { text: 'Follow', onclick: async () => { await fy.follow(c.author).catch((e) => status(String(e), 'error')); changedFollows = true; reloadSettings(); } }))));
    } else {
      const input = h('input', { type: 'text', placeholder: '0000-0000-0000-0000', spellcheck: 'false' }) as HTMLInputElement;
      const problem = h('p.error', { hidden: true });
      const btn: HTMLElement = h('button', { text: 'Connect', onclick: async () => {
        if (!input.value.trim()) return;
        btn.textContent = 'Connecting…'; (btn as HTMLButtonElement).disabled = true; problem.hidden = true;
        try { await fy.connectOrcid(input.value); changedFollows = true; await reloadSettings(); }
        catch (e) { problem.textContent = String(e); problem.hidden = false; btn.textContent = 'Connect'; (btn as HTMLButtonElement).disabled = false; }
      } });
      input.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); btn.click(); } });
      fill(orcidBox, h('div.row', {}, input, btn), problem,
        h('p.hint', { text: 'Optional. Lunta finds your papers on OpenAlex: papers citing them always show up, and your own never do.' }));
    }
  }

  function paintWatches() {
    fill(watchBox,
      ...s.watches.map((w) => h('div.row.fy-watch', {}, h('span', { text: w.describe }), h('div.spacer'),
        h('button.mini', { text: '×', title: 'Stop following', onclick: async () => { await fy.removeWatch(w.id).catch((e) => status(String(e), 'error')); reloadSettings(); } }))),
      h('button', { text: '＋ Follow a Journal, Author, Paper or Topic…', onclick: async () => { if (await followDialog()) { changedFollows = true; reloadSettings(); } } }),
      h('p.hint', { text: 'New papers by authors you follow, and papers citing ones you watch, always show up. Papers in journals you follow come first and pass at a lower bar.' }));
  }

  async function paintHighlights() {
    const [choices, j] = await Promise.all([fy.choices().catch(() => [] as JournalChoice[]), fy.highlights().catch(() => null)]);
    const cadence = h('select', { onchange: async (e: Event) => { await fy.cadence((e.target as HTMLSelectElement).value).catch((err) => status(String(err), 'error')); } },
      h('option', { value: 'weekly', text: 'Every week, on Monday morning', selected: j?.cadence !== 'daily' }),
      h('option', { value: 'daily', text: 'Every morning', selected: j?.cadence === 'daily' })) as HTMLSelectElement;
    const other = h('input', { type: 'text', spellcheck: 'false', placeholder: 'Another journal, e.g. Cell Systems' }) as HTMLInputElement;
    const problem = h('p.error', { hidden: true });
    const addOther: HTMLElement = h('button', { text: 'Add', onclick: async () => {
      if (!other.value.trim()) return;
      addOther.textContent = 'Looking…'; (addOther as HTMLButtonElement).disabled = true; problem.hidden = true;
      try { await fy.setJournal(other.value, true); other.value = ''; await paintHighlights(); }
      catch (e) { problem.textContent = String(e); problem.hidden = false; addOther.textContent = 'Add'; (addOther as HTMLButtonElement).disabled = false; }
    } });
    fill(hlBox,
      h('div.fy-grid', {}, h('label', { text: 'Papers come' }), cadence),
      h('div.fy-hl-choices', {}, ...choices.map((c) => h('label.fy-hl-choice', {},
        h('input', { type: 'checkbox', checked: c.on, onchange: async (e: Event) => {
          const on = (e.target as HTMLInputElement).checked;
          try { await fy.setJournal(c.name, on); } catch (err) { status(String(err), 'error'); }
          paintHighlights();
        } }),
        h('span', { text: c.name }), c.followed ? h('span.fy-dim', { text: ' · you follow it' }) : null))),
      h('div.row', {}, other, addOther), problem,
      h('p.hint', { text: 'The latest research papers in these journals, whatever their topic (no news, editorials or corrections), with a few sentences on the notable ones when the on-device model is downloaded. Each sentence cites the papers it is about, and says only what their titles and abstracts say.' }));
  }

  paintOrcid(); paintWatches(); paintHighlights();
  const done = () => back.remove();
  const save = async () => {
    const [hh, mm] = (morning.value || '07:00').split(':').map(Number);
    try {
      const newKey = key.value.trim() !== s.openalex_key;
      if (newKey) await fy.setKey(key.value);
      await fy.save(text.value, Math.max(3, Math.min(20, Number(perDay.value) || 8)), hh * 60 + mm, preprints.value);
      // connected or followed something new without changing the paragraph, or a key after a
      // failed run: look now too
      if (changedFollows || (newKey && key.value.trim() && feed?.fetch_failed)) await fy.run();
    } catch (e) { status(String(e), 'error'); return; }
    done();
    if (showing) reloadFeed();
  };
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) done(); } },
    h('div.modal.wide.fy-settings', {},
      h('h3', { text: 'New for You' }),
      h('h4', { text: 'Your research' }), text,
      h('div.row', {}, draftBtn),
      h('p.hint', { text: 'A few sentences on your topics, methods and systems. Lunta turns them into searches and uses them to rank and screen new papers.' + (s.ai ? '' : ' With the on-device model (Settings ▾ › On-Device AI…) it also reads the best abstracts and summarizes them.') }),
      h('h4', { text: 'Your ORCID iD' }), orcidBox,
      h('h4', { text: 'Every day' }),
      h('div.fy-grid', {},
        h('label', { text: 'Papers are picked at' }), morning,
        h('label', { text: 'Up to (papers a day)' }), perDay,
        h('label', { text: 'Papers from' }), preprints),
      h('p.hint', { text: 'The day’s papers are picked at this time on whichever device runs Lunta first (or as soon as Lunta opens after it). “Mostly journals” allows about one preprint in four.' }),
      h('h4', { text: 'Also follow' }), watchBox,
      h('h4', { text: 'From the journals you follow' }), hlBox,
      h('h4', { text: 'OpenAlex' }),
      h('div.row', {}, key, h('button', { text: 'Get a Free Key', onclick: () => openUrl('https://openalex.org/settings/api') })),
      h('p.hint', { text: 'New for You searches OpenAlex. Without a key, OpenAlex gives everyone on your network one small daily allowance, which a university network can use up by morning. A free key gives Lunta its own.' }),
      h('div.buttons', {}, h('button', { text: 'Cancel', onclick: done }), h('button.primary', { text: 'Save', onclick: save }))));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') done(); });
  document.body.append(back);
  text.focus();
}

/** Follow a journal, author, paper or topic (NewWatchView). True when one was added. */
function followDialog(): Promise<boolean> {
  return new Promise((resolve) => {
    let kind = 'journal';
    const input = h('input', { type: 'text', spellcheck: 'false', placeholder: KINDS[0][2] }) as HTMLInputElement;
    const problem = h('p.error', { hidden: true });
    const seg = h('div.fy-seg', {}, KINDS.map(([k, label, ph]) => h('button' + (k === kind ? '.on' : ''), { type: 'button', text: label, onclick: (e: Event) => {
      kind = k; input.placeholder = ph;
      seg.querySelectorAll('button').forEach((b) => b.classList.toggle('on', b === e.currentTarget));
      input.focus();
    } })));
    const ok: HTMLElement = h('button.primary', { type: 'submit', text: 'Follow' });
    const done = (v: boolean) => { back.remove(); resolve(v); };
    const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) done(false); } },
      h('form.modal', { onsubmit: async (e: Event) => {
        e.preventDefault();
        if (!input.value.trim()) return;
        ok.textContent = 'Looking…'; (ok as HTMLButtonElement).disabled = true; problem.hidden = true;
        try { await fy.addWatch(kind, input.value); done(true); }
        catch (err) { problem.textContent = String(err); problem.hidden = false; ok.textContent = 'Follow'; (ok as HTMLButtonElement).disabled = false; }
      } },
        h('h3', { text: 'Follow' }), seg, input, problem,
        h('p.hint', { text: 'Tip: right-click a paper in your library › Watch for Papers Citing This.' }),
        h('div.buttons', {}, h('button', { type: 'button', text: 'Cancel', onclick: () => done(false) }), ok)));
    back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') done(false); });
    document.body.append(back);
    input.focus();
  });
}

/** Context-menu items to start following from a library paper (WatchMenu). */
export function watchItems(p: { meta: { doi: string; title: string; authors: string[] } }): { label: string; action: () => void }[] {
  const items: { label: string; action: () => void }[] = [];
  const follow = (kind: string, value: string, what: string) => async () => {
    try { await fy.addWatch(kind, value); status(`New for You follows ${what}`); refreshCount(); }
    catch (e) { status(String(e), 'error'); }
  };
  if (p.meta.doi) items.push({ label: 'Watch for Papers Citing This', action: follow('citing', p.meta.doi, 'papers citing it') });
  const a = p.meta.authors;
  if (a.length) items.push({ label: `Follow ${a[0]}`, action: follow('author', a[0], a[0]) });
  if (a.length > 1) items.push({ label: `Follow ${a[a.length - 1]}`, action: follow('author', a[a.length - 1], a[a.length - 1]) });
  return items;
}
