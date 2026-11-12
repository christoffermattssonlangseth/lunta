// The two edit dialogs: paper details (MetadataEditor.swift) and smart folders (SmartFolderEditor
// in SmartFolders.swift), with the same fields and wording.
import { h } from './ui';
import { metaApi, smartApi, type Paper, type PaperMeta, type SmartFolder, type Folder } from './api';

function modal(title: string, body: HTMLElement[], onSave: () => Promise<void> | void, onClose: () => void, saveLabel = 'Save') {
  const note = h('div.note');
  const save = h('button.primary', { type: 'submit', text: saveLabel }) as HTMLButtonElement;
  const close = () => { back.remove(); onClose(); };
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) close(); } },
    h('form.modal.editor', {
      onsubmit: async (e: Event) => {
        e.preventDefault();
        save.disabled = true;
        try { await onSave(); back.remove(); onClose(); }
        catch (err) { note.textContent = String(err); save.disabled = false; }
      },
    }, h('h3', { text: title }), ...body, note,
    h('div.buttons', {}, h('button', { type: 'button', text: 'Cancel', onclick: close }), save)));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') close(); });
  document.body.append(back);
  return { note, back, save };
}

const field = (label: string, el: HTMLElement) => h('label.f', {}, label, el);
const text = (v: string, placeholder = '') => h('input', { type: 'text', value: v, placeholder, spellcheck: 'false' }) as HTMLInputElement;
const area = (v: string, rows: number) => { const a = h('textarea', { rows }) as HTMLTextAreaElement; a.value = v; return a; };

/** Edit a paper's details; saves through the core. Resolves true when saved. */
export function editDetails(p: Paper): Promise<boolean> {
  return new Promise((resolve) => {
    const m = p.meta;
    const title = area(m.title, 2), authors = area(m.authors.join('\n'), 4), journal = text(m.journal), date = text(m.date);
    const volume = text(m.volume), issue = text(m.issue), pages = text(m.pages), publisher = text(m.publisher);
    const doi = text(m.doi), pmid = text(m.pmid), pmcid = text(m.pmcid), arxiv = text(m.arxiv), url = text(m.url);
    const abstract = area(m.abstract_text, 6), keywords = text(m.keywords.join(', '));
    let saved = false;
    const lookup = h('button', { type: 'button', text: 'Fill In from the DOI (online)' }) as HTMLButtonElement;
    const read = (): PaperMeta => ({
      ...m,
      title: title.value.trim(),
      authors: authors.value.split('\n').map((s) => s.trim()).filter(Boolean),
      journal: journal.value, date: date.value.trim(), volume: volume.value, issue: issue.value, pages: pages.value,
      publisher: publisher.value, doi: doi.value.trim(), pmid: pmid.value.trim(), pmcid: pmcid.value.trim(),
      arxiv: arxiv.value.trim(), url: url.value.trim(), abstract_text: abstract.value,
      keywords: keywords.value.split(',').map((s) => s.trim()).filter(Boolean),
    });
    const { note } = modal('Edit Details', [
      field('Title', title),
      field('Authors, one per line', authors),
      h('h4', { text: 'Publication' }),
      field('Journal', journal),
      h('div.cols', {}, field('Date or year', date), field('Volume', volume), field('Issue', issue), field('Pages', pages)),
      field('Publisher', publisher),
      h('h4', { text: 'Identifiers' }),
      h('div.cols', {}, field('DOI', doi), field('PubMed ID', pmid)),
      h('div.cols', {}, field('PMCID', pmcid), field('arXiv', arxiv)),
      field('Link', url),
      h('h4', { text: 'Abstract' }), abstract,
      field('Keywords, comma separated', keywords),
      lookup,
    ], async () => {
      const next = read();
      if (!next.title) throw 'A paper needs a title.';
      await metaApi.update(p.id, next);
      saved = true;
    }, () => resolve(saved));
    title.focus();
    lookup.onclick = async () => {
      if (!doi.value.trim()) { note.textContent = 'Enter a DOI first.'; return; }
      lookup.disabled = true; note.textContent = 'Looking up the DOI…';
      try {
        const r = await metaApi.lookup(doi.value.trim());
        if (!r.title) { note.textContent = 'No record found for that DOI.'; return; }
        const keep = abstract.value;
        title.value = r.title; authors.value = r.authors.join('\n'); journal.value = r.journal; date.value = r.date;
        volume.value = r.volume; issue.value = r.issue; pages.value = r.pages; publisher.value = r.publisher;
        doi.value = r.doi || doi.value; pmid.value = r.pmid || pmid.value; pmcid.value = r.pmcid || pmcid.value;
        arxiv.value = r.arxiv || arxiv.value; url.value = r.url || url.value;
        abstract.value = r.abstract_text || keep; if (r.keywords.length) keywords.value = r.keywords.join(', ');
        note.textContent = 'Filled in from the DOI. Check, then Save.';
      } catch (e) { note.textContent = `No record found: ${e}`; }
      finally { lookup.disabled = false; }
    };
  });
}

export const blankSmart = (): SmartFolder => ({
  id: '', name: '', text: '', tag: '', folder: '', unreadOnly: false, starredOnly: false, yearFrom: '', yearTo: '',
  addedDays: 0, pdf: 'any', highlightsOnly: false,
});

const examples = (): SmartFolder[] => [
  { ...blankSmart(), name: 'Added this month', addedDays: 31 },
  { ...blankSmart(), name: 'Unread, last 3 years', unreadOnly: true, yearFrom: String(new Date().getFullYear() - 2) },
  { ...blankSmart(), name: 'No PDF yet', pdf: 'missing' },
  { ...blankSmart(), name: 'Highlighted', highlightsOnly: true },
];

/** "unread · #vbt · 2020–" (SmartFolder.summary). */
export function summary(f: SmartFolder): string {
  const parts: string[] = [];
  if (f.text) parts.push(`“${f.text}”`);
  if (f.unreadOnly) parts.push('unread');
  if (f.starredOnly) parts.push('starred');
  if (f.tag) parts.push(`#${f.tag}`);
  if (f.yearFrom || f.yearTo) parts.push(`${f.yearFrom}–${f.yearTo}`);
  if (f.addedDays > 0) parts.push(`added in the last ${f.addedDays} days`);
  if (f.pdf === 'has') parts.push('with PDF');
  if (f.pdf === 'missing') parts.push('no PDF');
  if (f.highlightsOnly) parts.push('highlighted');
  return parts.length ? parts.join(' · ') : 'all papers';
}

/** Create (f.id empty) or edit a smart folder. Resolves to the saved folder's id, or null. */
export function editSmart(f0: SmartFolder, tags: string[], folders: Folder[]): Promise<string | null> {
  return new Promise((resolve) => {
    const isNew = !f0.id;
    let f: SmartFolder = { ...f0 };
    let result: string | null = null;
    const name = text(f.name, 'Name');
    const words = text(f.text), yearFrom = text(f.yearFrom, 'from'), yearTo = text(f.yearTo, 'to');
    const check = (label: string, get: () => boolean, set: (v: boolean) => void) => {
      const c = h('input', { type: 'checkbox', checked: get() }) as HTMLInputElement;
      c.onchange = () => { set(c.checked); refresh(); };
      return { box: h('label.chk', {}, c, label), c, get };
    };
    const unread = check('Unread only', () => f.unreadOnly, (v) => (f.unreadOnly = v));
    const starred = check('Starred only', () => f.starredOnly, (v) => (f.starredOnly = v));
    const hl = check('With highlights only', () => f.highlightsOnly, (v) => (f.highlightsOnly = v));
    const select = (opts: [string, string][], value: string) => {
      const s = h('select') as HTMLSelectElement;
      for (const [v, l] of opts) s.append(h('option', { value: v, text: l }));
      s.value = value; s.onchange = refresh; return s;
    };
    const tag = select([['', 'Any'], ...tags.map((t) => [t, `#${t}`] as [string, string])], f.tag);
    const depth = (x: Folder) => { let d = 0, c = x.parent; while (c && d < 20) { c = folders.find((y) => y.id === c)?.parent ?? null; d++; } return d; };
    // folders in tree order, indented
    const order: Folder[] = [];
    const walk = (parent: string | null) => folders.filter((x) => (x.parent && folders.some((y) => y.id === x.parent) ? x.parent : null) === parent)
      .forEach((x) => { order.push(x); walk(x.id); });
    walk(null);
    const inFolder = select([['', 'Any'], ...order.map((x) => [x.id, '  '.repeat(depth(x)) + x.name] as [string, string])], f.folder);
    const added = select([['0', 'Any time'], ['7', 'Last 7 days'], ['31', 'Last 31 days'], ['90', 'Last 90 days'], ['365', 'Last year']], String(f.addedDays));
    const pdf = select([['any', 'Any'], ['has', 'Has a PDF'], ['missing', 'No PDF yet']], f.pdf);
    const count = h('div.count');
    const collect = () => {
      f = { ...f, name: name.value, text: words.value, tag: tag.value, folder: inFolder.value, yearFrom: yearFrom.value.trim(),
        yearTo: yearTo.value.trim(), addedDays: parseInt(added.value, 10) || 0, pdf: pdf.value as SmartFolder['pdf'] };
    };
    let serial = 0, timer: number | undefined;
    function refresh() {
      collect();
      clearTimeout(timer);
      timer = window.setTimeout(async () => {
        const s = ++serial;
        const n = await smartApi.count(f).catch(() => -1);
        if (s === serial && n >= 0) count.textContent = `${n} paper${n === 1 ? '' : 's'} match right now`;
      }, 200);
    }
    for (const el of [words, yearFrom, yearTo]) el.oninput = refresh;
    const start = isNew ? h('div.examples', {}, examples().map((ex) => h('button', {
      type: 'button', text: ex.name, onclick: () => {
        f = { ...ex, id: f.id };
        name.value = f.name; words.value = f.text; yearFrom.value = f.yearFrom; yearTo.value = f.yearTo;
        tag.value = f.tag; inFolder.value = f.folder; added.value = String(f.addedDays); pdf.value = f.pdf;
        for (const k of [unread, starred, hl]) k.c.checked = k.get();
        refresh();
      } }))) : null;
    modal(isNew ? 'New Smart Folder' : 'Edit Smart Folder', [
      field('Name', name),
      isNew ? h('h4', { text: 'Start from' }) : null, start,
      h('h4', { text: 'Papers that match' }),
      field('Words (title, authors, full text, notes)', words),
      unread.box, starred.box, hl.box,
      h('div.cols', {}, field('Tag', tag), field('In folder', inFolder)),
      h('div.cols', {}, field('Year from', yearFrom), field('Year to', yearTo)),
      h('div.cols', {}, field('Added', added), field('PDF', pdf)),
      count,
    ].filter(Boolean) as HTMLElement[], async () => {
      collect();
      if (!f.name.trim()) f.name = summary(f).replace(/^./, (c) => c.toUpperCase());
      f.name = f.name.trim();
      result = await smartApi.save(f);
    }, () => { clearTimeout(timer); resolve(result); });
    name.focus();
    refresh();
  });
}
