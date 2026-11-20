// Outline from Highlights…, as in the Apple and Android apps: the highlights and notes in some
// papers (a folder, a tag, the selected papers, the search results) as an outline by paper and
// section, or grouped into themes by the on-device AI. Every point is a highlight word for word
// or the researcher's own note (marked Note), cited author–year with the citation key; the model
// only groups the points and titles the themes. Copy Markdown or Save as .md. The work is the
// core's (src-tauri/src/tidy.rs outline_make → core/src/outline.rs).
import { invoke } from '@tauri-apps/api/core';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';
import { h, status, copy } from './ui';
import './tidy.css';

interface Outline { title: string; markdown: string; html: string; themes: unknown[]; items: number; papers: number; by_model: boolean; note: string; }

let openPaper: (id: string) => void = () => {};
/** A lunta://open link in the outline opens the paper. */
export function setOutlineOpener(f: (id: string) => void) { openPaper = f; }

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
  let making = false;
  const copyBtn = h('button', { type: 'button', text: 'Copy Markdown', disabled: true, onclick: () => outline && copy(outline.markdown, 'the outline as Markdown') }) as HTMLButtonElement;
  const saveBtn = h('button', { type: 'button', text: 'Save as .md…', disabled: true, onclick: async () => {
    if (!outline) return;
    const path = await saveDialog({ defaultPath: `${outline.title.replace(/[\\/:*?"<>|]/g, '-')}.md`, filters: [{ name: 'Markdown', extensions: ['md'] }] });
    if (typeof path !== 'string') return;
    invoke('save_report', { path, text: outline.markdown }).then(() => status(`Saved ${path.split(/[\\/]/).pop()}.`)).catch((e) => status(String(e), 'error'));
  } }) as HTMLButtonElement;
  const makeBtn = h('button.primary', { type: 'submit', text: 'Make Outline' }) as HTMLButtonElement;

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
      preview.hidden = false;
      const lines = [outline.note];
      if (outline.by_model) lines.push('The themes’ titles and summaries are the on-device AI’s (Qwen3 4B): check them. Every point is your highlight or note, word for word.');
      else if (ai) lines.push('The on-device AI gave no usable themes: by paper and section instead.');
      note.textContent = lines.filter(Boolean).join(' ');
      copyBtn.disabled = saveBtn.disabled = !outline.items;
    } catch (e) { status(String(e), 'error'); }
    finally { making = false; makeBtn.disabled = false; }
  }

  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) close(); } },
    h('form.modal.wide.check-modal.outline-modal', { onsubmit: (e: Event) => { e.preventDefault(); make(); } },
      h('h3', { text: 'Outline from Highlights' }),
      h('p.hint', { text: `The highlights and notes in ${what}, by paper and section, each cited with a link to the paper and its citation key.` }),
      topic,
      modelReady
        ? h('label.chk', {}, useModel, 'Group into themes with on-device AI (it titles the themes; the points stay your highlights)')
        : h('p.hint', { text: 'With On-Device AI (Settings ▾ › On-Device AI…), the highlights can be grouped into themes.' }),
      preview, note,
      h('div.buttons', {}, copyBtn, saveBtn,
        h('button', { type: 'button', text: 'Done', onclick: () => close() }), makeBtn)));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') close(); });
  document.body.append(back);
  topic.focus();
}
