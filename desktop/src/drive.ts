// Library Storage (Windows, Linux): where the library lives — Google Drive, synced through the
// Drive API (src-tauri/src/drivesync.rs, the core's Syncer, as on Android and the Apple apps), or a
// folder on this computer (as before; may be inside Google Drive for desktop, Dropbox, OneDrive).
// The sidebar's sync line, the Library Storage dialog, the first-run choice, Download / Remove
// Download for papers, and syncing when the window comes to the front.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open as openDialog, ask } from '@tauri-apps/plugin-dialog';
import { h, fill, status, type MenuItem } from './ui';
import type { Paper } from './api';

export interface DriveStatus {
  available: boolean; storage: 'drive' | 'folder'; library: string | null;
  signed_in: boolean; signing_in: boolean; syncing: boolean; last_sync_ms: number;
  error: string | null; notice: string | null; folder_not_visible: boolean; narrow: boolean;
  needs_sign_in: boolean; full_access: boolean;
  download_on_open: boolean; pending_uploads: number; pending_downloads: number; not_downloaded: number;
  here_bytes: number; drive_only_bytes: number; progress: string | null; dev_remote: string | null;
}

export const driveApi = {
  status: () => invoke<DriveStatus>('drive_status'),
  signIn: (pick = false) => invoke<void>('drive_sign_in', { pick }),
  cancelSignIn: () => invoke<void>('drive_cancel_sign_in'),
  signOut: () => invoke<void>('drive_sign_out'),
  syncNow: () => invoke<DriveStatus>('drive_sync_now'),
  syncSoon: () => invoke<void>('drive_sync_soon'),
  setDownloadOnOpen: (on: boolean) => invoke<void>('drive_set_download_on_open', { on }),
  remoteOnly: () => invoke<string[]>('drive_remote_only'),
  download: (ids: string[]) => invoke<number>('drive_download', { ids }),
  removeDownload: (ids: string[]) => invoke<string>('drive_remove_download', { ids }),
  move: (to: 'drive' | 'folder', folder?: string) => invoke<string>('storage_move', { to, folder: folder ?? null }),
  useDrive: () => invoke<void>('storage_use_drive'),
};

const CANCELLED = 'Google sign-in was cancelled.';
let st: DriveStatus | null = null;
let remoteOnly = new Set<string>();
let onChange: () => void = () => {};
const line = h('div.sync-line');

export function inDrive() { return st?.storage === 'drive'; }

/** Papers whose PDF, reading view or narration is only in Google Drive (not downloaded). */
export function isRemoteOnly(id: string) { return remoteOnly.has(id); }

/** The list's mark for a paper not downloaded. */
export function cloudMark(id: string): HTMLElement | null {
  return remoteOnly.has(id) ? h('span.badge.cloud', { text: '☁', title: 'In Google Drive, not downloaded: it downloads when you open it' }) : null;
}

/** Download / Remove Download, for a paper's context menu (a library synced on demand only). */
export function paperDriveItems(ps: Paper[]): MenuItem[] {
  if (!st || st.storage !== 'drive' || !st.signed_in || !st.download_on_open) return [];
  const ids = ps.map((p) => p.id);
  const some = ids.some((id) => remoteOnly.has(id));
  return ['-',
    { label: ids.length > 1 ? 'Download from Google Drive' : 'Download', disabled: !some, action: async () => {
      status('Downloading from Google Drive…', 'busy', true);
      try { const n = await driveApi.download(ids); status(n ? `Downloaded ${n} file${n === 1 ? '' : 's'}.` : 'Already downloaded.'); }
      catch (e) { status(String(e), 'error'); }
      await refreshState();
    } },
    { label: 'Remove Download', disabled: ids.every((id) => remoteOnly.has(id)), action: async () => {
      try { status(await driveApi.removeDownload(ids)); } catch (e) { status(String(e), 'error'); }
      await refreshState();
    } },
  ];
}

function ago(ms: number): string {
  if (!ms) return 'not synced yet';
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return 'synced just now';
  if (s < 3600) return `synced ${Math.round(s / 60)} min ago`;
  if (s < 86400) return `synced ${Math.round(s / 3600)} h ago`;
  return `synced ${new Date(ms).toLocaleDateString()}`;
}

function bytes(b: number): string {
  if (b >= 1 << 30) return `${(b / (1 << 30)).toFixed(1)} GB`;
  if (b >= 1 << 20) return `${Math.round(b / (1 << 20))} MB`;
  return `${Math.ceil(b / 1024)} KB`;
}

/** One line about the sync, for the sidebar and the dialog. */
function summary(s: DriveStatus): { text: string; error: boolean } {
  if (s.storage !== 'drive') return { text: '', error: false };
  if (!s.signed_in) return { text: 'Google Drive: signed out — changes stay on this computer', error: true };
  if (s.syncing) return { text: s.progress ? `Syncing… ${s.progress}` : 'Syncing with Google Drive…', error: false };
  if (s.needs_sign_in) return { text: 'Google Drive: sign in again', error: true };
  if (s.folder_not_visible) return { text: 'Google Drive: Lunta can’t open its folder', error: true };
  if (s.error) return { text: `Google Drive: ${s.error}`, error: true };
  return { text: `Google Drive: ${ago(s.last_sync_ms)}`, error: false };
}

function paintLine() {
  const s = st;
  if (!s || s.storage !== 'drive') { line.hidden = true; return; }
  const { text, error } = summary(s);
  line.hidden = false;
  line.className = 'sync-line' + (error ? ' error' : '') + (s.syncing ? ' busy' : '');
  line.textContent = text;
  line.title = 'Library Storage…';
  line.onclick = () => storageDialog();
}

/** The sidebar's last line (re-added each time the sidebar is drawn). */
export function syncLine(): HTMLElement { paintLine(); return line; }

async function refreshState() {
  try { st = await driveApi.status(); } catch { return; }
  remoteOnly = new Set(st.storage === 'drive' && st.download_on_open ? await driveApi.remoteOnly().catch(() => []) : []);
  paintLine();
  dialogRender?.();
}

// A library in a folder that Google Drive for desktop syncs: since 1.3 Lunta's Google Drive
// access covers only the files Lunta makes through its own connection (drive.file), so what Drive
// for desktop uploads from here no longer reaches Lunta on Android, a Mac or an iPhone.
const GDFD_HINT = 'This folder looks like one that Google Drive for desktop syncs. Lunta on Android, and on a Mac, iPhone or iPad with Google Drive, sees only the files Lunta itself puts into Google Drive, so what this computer adds here doesn’t reach them. Use Move to Google Drive… to sync through Lunta’s own connection.';
const GDFD_TOLD = 'driveForDesktopTold';

function looksLikeDriveForDesktop(s: DriveStatus): boolean {
  return s.storage === 'folder' && s.available && /my drive|google ?drive/i.test(s.library ?? '');
}

/** Once: tells a library in a Google Drive for desktop folder to move to Google Drive. */
async function tellAboutDriveForDesktop() {
  if (!st || !looksLikeDriveForDesktop(st) || localStorage.getItem(GDFD_TOLD)) return;
  localStorage.setItem(GDFD_TOLD, '1');
  const open = await ask(`${GDFD_HINT}\n\nSettings ▾ › Library Storage… › Move to Google Drive…`,
    { title: 'Your library is in Google Drive for desktop', okLabel: 'Library Storage…', cancelLabel: 'Later' });
  if (open) storageDialog();
}

/** Starts listening to the sync (call once the library is shown). `changed` reloads the lists. */
export async function startDrive(changed: () => void) {
  onChange = changed;
  await refreshState();
  tellAboutDriveForDesktop().catch(() => {});
  listen<{ syncing: boolean; changed: boolean }>('drive-sync', async (e) => {
    await refreshState();
    if (e.payload.changed) onChange();
  });
  listen<string>('drive-progress', (e) => { if (st) { st.progress = e.payload; paintLine(); dialogProgress?.(e.payload); } });
  let downloading = 0;
  listen<{ id: string; active: boolean }>('drive-download', (e) => {
    downloading += e.payload.active ? 1 : -1;
    if (downloading > 0) status('Downloading from Google Drive…', 'busy', true);
    else { status(''); refreshState(); }
  });
  // back at the window: sync if the last one is over a minute old
  window.addEventListener('focus', () => { if (st?.storage === 'drive') driveApi.syncSoon().catch(() => {}); });
  // "synced 3 min ago" stays true
  setInterval(() => { if (!st?.syncing) refreshState(); }, 30000);
}

/** Signs in in the browser, with a dialog to cancel. Resolves true when signed in. */
export function signIn(pick = false): Promise<boolean> {
  return new Promise((resolve) => {
    const msg = h('p.hint', { text: 'Your browser opens Google’s sign-in. Choose your account and allow Lunta to use Google Drive, then come back here.' });
    const note = h('p.hint', { text: 'Lunta asks only for the files it makes itself: its “Lunta” folder in your Drive, nothing else there.' });
    const close = (ok: boolean) => { back.remove(); resolve(ok); };
    const back = h('div.modal-back', {},
      h('div.modal', {},
        h('h3', { text: pick ? 'Choose your Lunta folder' : 'Sign in to Google Drive' }), msg, note,
        h('div.buttons', {}, h('button', { text: 'Cancel', onclick: () => { driveApi.cancelSignIn(); close(false); } }))));
    document.body.append(back);
    driveApi.signIn(pick).then(async () => { await refreshState(); close(true); })
      .catch((e) => {
        if (String(e).includes(CANCELLED)) return;   // closed by Cancel
        back.remove();
        status(String(e), 'error');
        resolve(false);
      });
  });
}

// ---- Library Storage… ----

let dialogRender: (() => void) | null = null;
let dialogProgress: ((line: string) => void) | null = null;

export async function storageDialog() {
  await refreshState();
  let message = '', isError = false, busy = false, progress = '';
  const body = h('div.storage-body');
  const close = () => { dialogRender = null; dialogProgress = null; back.remove(); };
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back && !busy) close(); } },
    h('div.modal.wide', {},
      h('h3', { text: 'Library Storage' }),
      body,
      h('div.buttons', {}, h('span', { style: 'flex:1' }), h('button.primary', { text: 'Done', onclick: () => { if (!busy) close(); } }))));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape' && !busy) close(); });
  document.body.append(back);
  dialogRender = render;
  dialogProgress = (l) => { if (busy) { progress = l; render(); } };

  async function run(label: string, f: () => Promise<string | void>, reload = false) {
    busy = true; message = label; isError = false; progress = ''; render();
    try { message = String((await f()) ?? ''); }
    catch (e) { message = String(e); isError = !String(e).includes(CANCELLED); if (!isError) message = ''; }
    busy = false; progress = '';
    await refreshState();
    if (reload) onChange();
    render();
  }

  const button = (label: string, action: () => void, primary = false, disabled = false) =>
    h(primary ? 'button.primary' : 'button', { text: label, disabled: disabled || busy, onclick: action });

  async function moveToDrive() {
    if (!st!.signed_in && !(await signIn())) return;
    const sure = await ask('Lunta copies your library into the “Lunta” folder in your Google Drive and syncs it from there, with Lunta on Android, on a Mac or iPhone whose library is in Google Drive, and on your other computers. If that folder already has a library, the two are merged. The library in its current folder stays as it is; Lunta stops using it.', { title: 'Move your library to Google Drive?', okLabel: 'Move', cancelLabel: 'Cancel' });
    if (sure) run('Moving your library to Google Drive…', () => driveApi.move('drive'), true);
  }

  async function moveToFolder() {
    const dir = await openDialog({ directory: true, title: 'Move the library to this folder' });
    if (typeof dir !== 'string') return;
    const from = st!.storage === 'drive'
      ? 'Lunta syncs once more, downloads the papers that are only in Google Drive, then copies the library into this folder. The copy in Google Drive stays there (your other devices keep using it); this computer stops syncing it.'
      : 'Lunta copies the library into this folder (merging with a library already there). The old folder stays as it is; Lunta stops using it.';
    const sure = await ask(`${from}\n\n${dir}`, { title: 'Move your library to this folder?', okLabel: 'Move', cancelLabel: 'Cancel' });
    if (sure) run('Moving your library…', () => driveApi.move('folder', dir), true);
  }

  function render() {
    const s = st;
    if (!s) { fill(body, h('p.hint', { text: 'Looking…' })); return; }
    const drive = s.storage === 'drive';
    const sum = summary(s);
    const choice = (on: boolean, title: string, text: string) =>
      h('div.storage-choice' + (on ? '.on' : ''), {}, h('span.storage-dot', { text: on ? '●' : '○' }),
        h('div', {}, h('b', { text: title }), h('div.hint', { text })));
    const unavailable = !s.available
      ? h('p.hint', { text: 'Google sign-in isn’t built into this copy of Lunta. Use a release from getlunta.app, or set LUNTA_GOOGLE_CLIENT_ID and LUNTA_GOOGLE_CLIENT_SECRET (see docs/development.md, Google Drive sync).' })
      : null;
    const driveSide = drive ? [
      h('p' + (sum.error ? '.cite-problem' : '.hint'), { text: sum.text }),
      s.dev_remote ? h('p.hint', { text: `Test mode: the folder ${s.dev_remote} stands in for Google Drive.` }) : null,
      s.notice ? h('p.hint', { text: s.notice }) : null,
      s.needs_sign_in && s.error ? h('p.cite-problem', { text: s.error }) : null,
      s.folder_not_visible
        ? h('p.cite-problem', { text: s.narrow
            ? 'Lunta can’t open its folder in Google Drive (made by another app, or access removed). Choose your Lunta folder in Google’s picker.'
            : 'Lunta can’t open its folder in Google Drive. Sign in again, or check that the “Lunta” folder is still in your Drive.' })
        : null,
      s.full_access && s.signed_in && !s.syncing && !s.error && s.last_sync_ms
        ? h('p.hint', { text: 'Lunta now asks Google only for access to the files it makes, not your whole Drive. The Drive folder has been prepared for it; sign in again to switch.' }) : null,
      s.signed_in && (s.pending_uploads || s.pending_downloads)
        ? h('p.hint', { text: `Waiting: ${s.pending_uploads} to upload, ${s.pending_downloads} to download.` }) : null,
      h('div.buttons.left', {},
        s.signed_in ? button('Sync Now', () => run('Syncing…', async () => { await driveApi.syncNow(); return 'Synced.'; }, true), true) : button('Sign In…', () => signIn().then(() => render()), true),
        s.signed_in && (s.needs_sign_in || (s.full_access && !s.error && s.last_sync_ms)) ? button('Sign In Again…', () => signIn().then(() => render())) : null,
        s.folder_not_visible && s.narrow ? button('Choose Your Lunta Folder…', () => signIn(true).then(() => render())) : null,
        s.signed_in && !s.dev_remote ? button('Sign Out', async () => {
          if (await ask('Lunta forgets your Google sign-in on this computer. Your library stays here and syncs again after you sign in; nothing is deleted in Google Drive.', { title: 'Sign out of Google Drive?', okLabel: 'Sign Out', cancelLabel: 'Cancel' })) {
            run('Signing out…', async () => { await driveApi.signOut(); return 'Signed out. Changes stay on this computer until you sign in again.'; });
          }
        }) : null),
      h('label.check', {},
        h('input', { type: 'checkbox', checked: s.download_on_open, disabled: busy, onchange: (e: Event) => {
          const on = (e.target as HTMLInputElement).checked;
          run(on ? 'Turning on…' : 'Downloading the rest at the next sync…', async () => { await driveApi.setDownloadOnOpen(on); }, true);
        } }),
        'Download papers when you open them'),
      h('p.hint', { text: s.download_on_open
        ? `PDFs, reading views and narration stay in Google Drive until you open them. Here: ${bytes(s.here_bytes)}; only in Google Drive: ${bytes(s.drive_only_bytes)}${s.not_downloaded ? ` (${s.not_downloaded} files)` : ''}.`
        : 'Every paper’s files are downloaded to this computer. Turn this on to keep PDFs, reading views and narration in Google Drive until you open them.' }),
      s.download_on_open && s.signed_in ? h('div.buttons.left', {},
        button('Download All', () => run('Downloading from Google Drive…', async () => { const n = await driveApi.download([]); return n ? `Downloaded ${n} file${n === 1 ? '' : 's'}.` : 'Everything is downloaded.'; }, true), false, !s.not_downloaded)) : null,
    ] : [];
    fill(body,
      h('p.hint', { text: `Library: ${s.library ?? '—'}` }),
      choice(drive, 'Google Drive', 'Syncs through your Google account, with Lunta on Android, on a Mac, iPhone or iPad whose library is in Google Drive, and on your other computers. Lunta keeps it in a “Lunta” folder in your Drive and uses nothing else there.'),
      ...driveSide,
      choice(!drive, 'A folder on this computer', 'Any folder; to share it, one that Google Drive for desktop, Dropbox or OneDrive syncs. Each computer writes only its own change log, so syncing never conflicts.'),
      looksLikeDriveForDesktop(s)
        ? h('p.hint', { text: GDFD_HINT })
        : null,
      unavailable,
      h('div.buttons.left', {},
        drive ? null : button('Move to Google Drive…', moveToDrive, false, !s.available),
        button(drive ? 'Move to a Folder on This Computer…' : 'Move to Another Folder…', moveToFolder)),
      h('p.hint', { text: 'Moving copies the library; the old copy is never changed or deleted.' }),
      busy ? h('p.hint', { text: progress || message }) : message ? h('p' + (isError ? '.cite-problem' : '.hint'), { text: message }) : null,
    );
  }
  render();
}

/** First run: the welcome card's Google Drive choice. Resolves true when the library is open. */
export async function chooseDriveAtStart(): Promise<boolean> {
  const s = await driveApi.status();
  if (!s.available) { status('Google sign-in isn’t built into this copy of Lunta.', 'error'); return false; }
  if (!s.signed_in && !(await signIn())) return false;
  try { await driveApi.useDrive(); await refreshState(); return true; }
  catch (e) { status(String(e), 'error'); return false; }
}

export async function driveAvailable(): Promise<boolean> {
  return driveApi.status().then((s) => s.available).catch(() => false);
}
