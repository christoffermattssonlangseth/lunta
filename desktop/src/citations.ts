// Citations in Word & Google Docs… (Windows, Linux), as on the Mac (apple/Mac/CitationsView.swift):
// trust Lunta's certificate, install the Word add-in, and see whether Word can reach Lunta. The app
// serves the add-in on https://localhost:47481 (src-tauri/src/citeserver.rs); the trust stores and
// Word's registration are in src-tauri/src/citesetup.rs.
import { invoke } from '@tauri-apps/api/core';
import { save as saveDialog, ask } from '@tauri-apps/plugin-dialog';
import { h, fill, copy } from './ui';

interface Status {
  platform: 'windows' | 'linux' | 'macos';
  serving: boolean;
  problem: string | null;
  port: number;
  trusted: boolean | null;
  wordInstalled: boolean | null;
  lastRequest: number | null;
  certutil: boolean;
  token: string;
  caPath: string;
}

export async function citationsDialog() {
  let st: Status | null = null;
  let message = '', isError = false, busy = false;
  const body = h('div.citations-body');
  const close = () => { clearInterval(timer); back.remove(); };
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) close(); } },
    h('div.modal.citations', {},
      h('h3', { text: 'Citations in Word & Google Docs' }),
      body,
      h('div.buttons', {},
        h('button', { text: 'Remove…', title: 'Take the add-in out of Word and stop trusting Lunta’s certificate', onclick: remove }),
        h('span', { style: 'flex:1' }),
        h('button.primary', { text: 'Done', onclick: () => close() }))));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') close(); });
  document.body.append(back);

  async function refresh() {
    try { st = await invoke<Status>('citations_status'); } catch (e) { message = String(e); isError = true; }
    render();
  }
  // "connected" follows Word's panel while the window is open
  const timer = window.setInterval(() => { if (!busy) refresh(); }, 4000);

  async function run(cmd: string, args: Record<string, unknown> = {}) {
    busy = true; message = 'Working…'; isError = false; render();
    try { message = String((await invoke<string>(cmd, args)) ?? ''); }
    catch (e) { message = String(e); isError = true; }
    busy = false;
    await refresh();
  }

  async function saveManifest() {
    const path = await saveDialog({ title: 'Save the Word add-in', defaultPath: 'lunta-citations.xml', filters: [{ name: 'Add-in manifest', extensions: ['xml'] }] });
    if (!path) return;
    try { await invoke('citations_save_manifest', { path }); message = `Saved ${path.split(/[\\/]/).pop()}. Upload it in Word on the web as described above.`; isError = false; }
    catch (e) { message = String(e); isError = true; }
    render();
  }

  async function saveCertificate() {
    const path = await saveDialog({ title: 'Save Lunta’s certificate', defaultPath: 'lunta-local-ca.pem', filters: [{ name: 'Certificate', extensions: ['pem', 'crt'] }] });
    if (!path) return;
    try { await invoke('citations_save_certificate', { path }); message = 'Saved. Import it as a certificate authority in the browser’s settings.'; isError = false; }
    catch (e) { message = String(e); isError = true; }
    render();
  }

  async function remove() {
    const sure = await ask('Lunta Citations is taken out of Word, and this computer stops trusting Lunta’s certificate (Windows asks you to confirm). Citations already in documents stay as they are.', { title: 'Remove citing in Word?', kind: 'warning', okLabel: 'Remove', cancelLabel: 'Cancel' });
    if (sure) await run('citations_remove');
  }

  async function googleFile(which: 'code' | 'sidebar') {
    const [code, sidebar] = await invoke<[string, string]>('citations_google_files');
    await copy(which === 'code' ? code : sidebar, which === 'code' ? 'Code.gs' : 'Sidebar.html');
  }

  function step(n: number, done: boolean | null, text: string | Node, ...actions: (Node | null)[]) {
    return h('div.cite-step', {},
      h('span.cite-n' + (done ? '.done' : ''), { text: done ? '✓' : String(n) }),
      h('div.cite-text', {}, text),
      h('div.cite-actions', {}, ...actions));
  }
  const button = (label: string, action: () => void, disabled = false, primary = false) =>
    h(primary ? 'button.primary' : 'button', { text: label, disabled: disabled || busy, onclick: action });

  function render() {
    if (!st) { fill(body, h('p.hint', { text: 'Looking…' })); return; }
    const s = st;
    const win = s.platform === 'windows';
    const connected = s.lastRequest !== null && s.lastRequest < 600;
    const server = s.platform === 'macos'
      ? h('p.cite-problem', { text: 'On a Mac, use Lunta for Mac’s Lunta › Citations in Word & Google Docs…; this app is for Windows and Linux.' })
      : s.problem
        ? h('p.cite-problem', { text: s.problem })
        : h('p.hint', { text: connected ? `Word is connected (it last asked ${s.lastRequest! < 60 ? 'just now' : `${Math.round(s.lastRequest! / 60)} min ago`}).` : `Lunta is ready for the add-in on https://localhost:${s.port}.` });

    const trust = win
      ? step(1, s.trusted, 'Let Word talk to Lunta securely: trust Lunta’s own certificate, for this computer and this Windows account only. Windows shows a security warning naming “Lunta Local CA”: choose Yes.',
          button(s.trusted ? 'Trusted' : 'Trust Certificate…', () => run('citations_trust'), !!s.trusted, !s.trusted))
      : step(1, s.trusted, s.certutil
          ? 'Let your browser talk to Lunta securely: trust Lunta’s own certificate (it is valid for this computer only) in Chrome/Chromium and Firefox. Restart the browser afterwards.'
          : 'Let your browser talk to Lunta securely: Save Certificate…, then import it as an authority: Chrome › Settings › Privacy and security › Security › Manage certificates › Authorities › Import; Firefox › Settings › Privacy & Security › View Certificates › Authorities › Import, and tick “Trust this CA to identify websites”. (With certutil, from libnss3-tools, Lunta does this for you.)',
          s.certutil ? button(s.trusted ? 'Trust Again' : 'Trust Certificate', () => run('citations_trust'), false, !s.trusted) : null,
          button('Save Certificate…', saveCertificate));

    const word = win
      ? [
          trust,
          step(2, s.wordInstalled, 'Add Lunta Citations to Word (for you only; no administrator needed).',
            button(s.wordInstalled ? 'Reinstall' : 'Install in Word', () => run('citations_install_word'), false, !!s.trusted && !s.wordInstalled)),
          step(3, null, 'Quit Word completely and open it again. Click Cite on the Home tab (or Home › Add-ins and pick Lunta Citations under Developer Add-ins). Search, pick papers, Insert Citation; Insert Bibliography where the references go.'),
        ]
      : [trust];

    const web = step(win ? 4 : 2, null,
      h('span', {}, 'Word on the web', win ? ' (also in Edge or Chrome on this computer)' : '', ': Save Manifest…, then in a document Home › Add-ins › More Settings › Upload My Add-in, and choose the saved file. Do it once in each browser.'),
      button('Save Manifest…', saveManifest));

    fill(body,
      h('p.hint', { text: 'Cite papers from your Lunta library while you write. Citations update when you change style, and the reference list is built for you (APA, Vancouver, Nature and nine more styles, or any of ~10,000 journal styles).' }),
      server,
      h('h4', { text: win ? 'Microsoft Word' : 'Word on the web' }),
      ...word,
      web,
      h('p.hint', { text: 'Lunta has to be open to search and cite. Citations already in a document stay as they are without it, and Update All works from the copy of the cited papers each document keeps.' }),
      h('h4', { text: 'Google Docs' }),
      h('p.cite-text', { text: 'With the Lunta browser extension (Chrome, Edge, Firefox), click the Lunta button in any Google Doc: nothing to set up here.' }),
      h('details', {},
        h('summary', { text: 'Without the extension: Apps Script in one document' }),
        step(1, s.trusted, 'Do step 1 above (the sidebar uses the same certificate).'),
        step(2, null, 'In your document: Extensions › Apps Script. Replace the code in Code.gs with this:', button('Copy Code.gs', () => googleFile('code'))),
        step(3, null, 'Click + › HTML, name it Sidebar, and replace its contents with this (it includes your pairing key):', button('Copy Sidebar.html', () => googleFile('sidebar'))),
        step(4, null, 'Save, reload the document, then use the new Lunta › Cite… menu. Allow the script when Google asks.'),
        h('p.hint', {}, 'Pairing key: ', h('code', { text: s.token }), ' ', h('button.link', { text: 'Copy', onclick: () => copy(s.token, 'the pairing key') }))),
      message ? h('p' + (isError ? '.cite-problem' : '.cite-message'), { text: message }) : null,
    );
  }

  render();
  await refresh();
}
