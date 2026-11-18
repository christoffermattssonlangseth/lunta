// Updates from GitHub Releases (src-tauri/src/updates.rs, tauri-plugin-updater). A quiet check at
// start and once a day while Lunta runs, when "Check for Updates Automatically" is on (Settings ▾);
// "Check for Updates…" asks at once. Windows and the Linux AppImage install and restart; a .deb or
// .rpm install gets Download (the release page) instead.
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { h, $, status } from './ui';

export interface UpdateInfo { version: string; current: string; notes: string; can_install: boolean; page: string; }

export const updatesApi = {
  check: () => invoke<UpdateInfo | null>('update_check'),
  install: () => invoke<void>('update_install'),
  auto: () => invoke<boolean>('update_auto'),
  setAuto: (on: boolean) => invoke<void>('set_update_auto', { on }),
};

const DAY = 24 * 60 * 60 * 1000;
let dismissed = '';   // the version the banner was closed for (not shown again this run)

/** Starts the automatic checks: now (a few seconds after start) and every day. */
export function startUpdateChecks() {
  const quiet = async () => {
    if (!(await updatesApi.auto().catch(() => false))) return;
    const u = await updatesApi.check().catch(() => null);   // offline, or no feed yet: say nothing
    if (u && u.version !== dismissed) banner(u);
  };
  window.setTimeout(quiet, 5000);
  window.setInterval(quiet, DAY);
}

/** Check for Updates…: says what it found, also "up to date" and errors. */
export async function checkNow() {
  status('Checking for updates…', 'busy');
  try {
    const u = await updatesApi.check();
    if (u) { status(''); details(u); }
    else status(`Lunta is up to date.`);
  } catch (e) { status(String(e), 'error'); }
}

async function install(u: UpdateInfo) {
  if (!u.can_install) { openUrl(u.page); return; }
  $('#update-banner')?.remove();
  status(`Downloading Lunta ${u.version}…`, 'busy');
  try { await updatesApi.install(); }   // restarts into the new version
  catch (e) { status(String(e), 'error'); }
}

function actionLabel(u: UpdateInfo) { return u.can_install ? 'Install and Restart' : 'Download'; }

/** The small prompt in the corner after a quiet check. */
function banner(u: UpdateInfo) {
  $('#update-banner')?.remove();
  const close = () => { dismissed = u.version; bar.remove(); };
  const bar = h('div.update-banner#update-banner', { role: 'status' },
    h('div.update-text', {}, h('strong', { text: `Lunta ${u.version} is available` }), h('span', { text: ` (you have ${u.current})` })),
    h('div.buttons', {},
      h('button', { text: 'What’s New', onclick: () => { bar.remove(); details(u); } }),
      h('button', { text: 'Later', onclick: close }),
      h('button.primary', { text: actionLabel(u), onclick: () => install(u) })));
  document.body.append(bar);
}

/** The dialog with the release notes. */
function details(u: UpdateInfo) {
  const done = () => back.remove();
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) done(); } },
    h('div.modal.wide', {},
      h('h3', { text: `Lunta ${u.version} is available` }),
      h('p.update-dim', { text: u.can_install
        ? `You have ${u.current}. Lunta downloads the new version from GitHub, checks its signature, installs it and restarts.`
        : `You have ${u.current}. This copy was installed with the system's package manager (.deb or .rpm): download the new package from the release page.` }),
      h('div.update-notes', {}, ...markdown(u.notes || 'See the release page for what’s new.')),
      h('div.buttons', {},
        h('button', { text: 'Release Page', onclick: () => openUrl(u.page) }),
        h('button', { text: 'Later', onclick: () => { dismissed = u.version; done(); } }),
        h('button.primary', { text: actionLabel(u), onclick: () => { done(); install(u); } }))));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') done(); });
  document.body.append(back);
}

/** The little Markdown release notes use: headings, bullets, paragraphs, **bold** and `code`. */
export function markdown(text: string): HTMLElement[] {
  const out: HTMLElement[] = [];
  let list: HTMLElement | null = null;
  for (const raw of text.replace(/\r/g, '').split('\n')) {
    const line = raw.trimEnd();
    const bullet = line.match(/^\s*[-*]\s+(.*)$/);
    if (bullet) {
      if (!list) out.push(list = h('ul'));
      list.append(h('li', {}, ...inline(bullet[1])));
      continue;
    }
    list = null;
    const head = line.match(/^#{1,6}\s+(.*)$/);
    if (head) out.push(h('h4', {}, ...inline(head[1])));
    else if (line.trim()) out.push(h('p', {}, ...inline(line)));
  }
  return out;
}

function inline(s: string): (Node | string)[] {
  return s.split(/(\*\*[^*]+\*\*|`[^`]+`)/).filter(Boolean).map((part) =>
    part.startsWith('**') && part.endsWith('**') && part.length > 4 ? h('strong', { text: part.slice(2, -2) })
      : part.startsWith('`') && part.endsWith('`') && part.length > 2 ? h('code', { text: part.slice(1, -1) })
        : part);
}
