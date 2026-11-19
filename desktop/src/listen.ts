// Listen: the paper read aloud, as the Apple apps do it (apple/Shared/Narrator.swift, NowPlaying.swift).
//
// The reading view's narration items (narration.js: headings, paragraphs, list items) are read one
// after the other; the one being read is highlighted and kept in view, and tapping a paragraph
// reads from there. Three ways to read:
//  - a narration made on the Mac ("Make Natural Narration": narration.m4a + narration.json next to
//    the paper), played with its timing map;
//  - the natural voice (Kokoro-82M, src-tauri/src/listen.rs), one sentence piece at a time, made a
//    few pieces ahead of what plays, so it starts within a second or two and never waits after that;
//  - the system's voice (the web view's speech synthesis: Windows' voices, speech-dispatcher on
//    Linux when WebKitGTK has it) until the natural voice is downloaded.
// The narration keeps going when the reader closes; the library then shows a Now Playing bar.
import { invoke } from '@tauri-apps/api/core';
import { listen as onEvent } from '@tauri-apps/api/event';
import { h, $, status, menu, type MenuItem } from './ui';
import './listen.css';

export interface NarrationItem { id: string; text: string; kind: string; }
interface SpokenItem { id: string; kind: string; pieces: string[]; }
interface VoiceInfo { id: string; name: string; }
interface VoiceStatus { installed: boolean; download_bytes: number; disk_bytes: number; }
interface Timings { voice: string; segments: { id: string; kind: string; start: number; end: number }[]; }

/** What the reader lends the narration while it is open. */
export interface ReaderLink {
  highlight(id: string): void;
  setListening(on: boolean): void;
}

const voiceApi = {
  voices: () => invoke<VoiceInfo[]>('listen_voices'),
  status: () => invoke<VoiceStatus>('listen_status'),
  download: () => invoke<VoiceStatus>('listen_download'),
  cancel: () => invoke<void>('listen_cancel_download'),
  remove: () => invoke<VoiceStatus>('listen_remove'),
  prepareText: (items: NarrationItem[]) => invoke<SpokenItem[]>('listen_prepare_text', { items, max: 300 }),
  generation: (generation: number) => invoke<void>('listen_generation', { generation }),
  warmUp: (voice: string) => invoke<void>('listen_warm_up', { voice }),
  clip: (text: string, voice: string, generation: number, before: number, after: number) =>
    invoke<ArrayBuffer>('listen_clip', { text, voice, generation, before, after }),
  paperTimings: (id: string) => invoke<string | null>('listen_paper_timings', { id }),
  paperAudio: (id: string) => invoke<ArrayBuffer>('listen_paper_audio', { id }),
};

const SPEEDS = [0.8, 1.0, 1.15, 1.3, 1.5, 1.75, 2.0];
/** Silence around an item, as the Mac's renderer has it (lunta_tts.py PAUSE). */
const PAUSE: Record<string, [number, number]> = { heading: [0.55, 0.45], caption: [0.2, 0.4], text: [0, 0.35] };
/** Pieces made ahead of the one playing. */
const AHEAD = 3;

type Mode = 'natural' | 'system' | 'recorded';

// ---- state ----

let items: SpokenItem[] = [];
let index = 0;          // item being read
let piece = 0;          // piece of it (natural and system voices)
let playing = false;
let waiting = false;    // the natural voice is still making the next piece
let mode: Mode = 'system';
let title = '';
let paperId = '';
let speed = Number(localStorage.getItem('narrationSpeed') || '1') || 1;
let naturalVoice = localStorage.getItem('neuralVoice') || 'af_heart';
let systemVoice = localStorage.getItem('narrationVoice') || '';
let link: ReaderLink | null = null;
let note = '';
let voiceStatus: VoiceStatus | null = null;
let downloading: { done: number; total: number } | null = null;
let voices: VoiceInfo[] = [];
let generation = 0;
let timings: Timings['segments'] = [];

const audio = new Audio();
audio.preservesPitch = true;
/** Pieces made by the natural voice: key "<item>:<piece>" → blob URL (null: nothing to say). */
const clips = new Map<string, Promise<string | null>>();
let producing = false;
let utterance: SpeechSynthesisUtterance | null = null;
/** Called to open a paper from the Now Playing bar (main.ts sets it). */
let openPaper: ((id: string) => void) | null = null;
const listeners = new Set<() => void>();

export const narrationActive = () => items.length > 0;
export const narrationPaper = () => (items.length ? paperId : '');
export function setOpenPaper(f: (id: string) => void) { openPaper = f; }
export function onNarrationChange(f: () => void) { listeners.add(f); }

function changed() {
  renderBars();
  updateMediaSession();
  listeners.forEach((f) => f());
}

// ---- starting and stopping ----

/** Starts reading a paper: `pageItems` from the reading view (luntaNarration), `from` the id the reader is looking at. */
export async function startNarration(id: string, paperTitle: string, pageItems: NarrationItem[], from: string, reader: ReaderLink, hasAudio: boolean) {
  stop();
  paperId = id;
  title = paperTitle;
  link = reader;
  note = '';
  if (!pageItems.length) { status('Nothing to read aloud in this paper.', 'error'); return; }
  // a narration made on the Mac: its audio and timing map
  if (hasAudio && await loadRecorded(id, pageItems)) {
    mode = 'recorded';
  } else {
    items = (await voiceApi.prepareText(pageItems)).filter((i) => i.pieces.length);
    voiceStatus = await voiceApi.status().catch(() => null);
    mode = voiceStatus?.installed ? 'natural' : 'system';
    if (mode === 'system' && !(await systemVoices()).length) {
      note = 'novoice';
    } else if (mode === 'system') {
      note = 'offer';
    }
    if (mode === 'natural') voiceApi.warmUp(naturalVoice).catch(() => {});
  }
  if (!items.length) { status('Nothing to read aloud in this paper.', 'error'); return; }
  link.setListening(true);
  const start = Math.max(0, items.findIndex((i) => i.id === from));
  if (note === 'novoice') { index = start; changed(); voiceDialog(); return; }
  play(start);
}

/** The reader closes: narration goes on, the library shows Now Playing. */
export function detachReader() {
  link = null;
  changed();
}

/** The reader of the paper being read opens again: the page follows the voice again. */
export function attachReader(id: string, reader: ReaderLink): boolean {
  if (!items.length || id !== paperId) return false;
  link = reader;
  reader.setListening(true);
  if (items[index]) reader.highlight(items[index].id);
  changed();
  return true;
}

export function stop() {
  playing = false;
  waiting = false;
  newGeneration();
  audio.pause();
  audio.removeAttribute('src');
  if (utterance) { utterance.onend = null; speechSynthesis.cancel(); utterance = null; }
  clearClips();
  link?.setListening(false);
  items = [];
  timings = [];
  index = 0;
  piece = 0;
  changed();
}

// ---- control ----

export function play(i = index) {
  if (!items.length) return;
  index = Math.max(0, Math.min(i, items.length - 1));
  piece = 0;
  playing = true;
  newGeneration();
  if (utterance) { utterance.onend = null; speechSynthesis.cancel(); utterance = null; }
  link?.highlight(items[index].id);
  if (mode === 'recorded') {
    audio.currentTime = timings[index].start;
    audio.playbackRate = speed;
    audio.play().catch((e) => status(`Couldn't play the narration: ${e}`, 'error'));
  } else if (mode === 'natural') {
    audio.pause();
    playPiece();
  } else {
    speakPiece();
  }
  changed();
}

export function playFromId(id: string) {
  const i = items.findIndex((x) => x.id === id);
  if (i >= 0) play(i);
}

export function toggle() {
  if (!items.length) return;
  if (playing) { pause(); return; }
  if (mode === 'system' && note === 'novoice') { voiceDialog(); return; }
  if (mode === 'system') { play(index); return; }     // speech starts the paragraph over
  if (audio.src && audio.currentTime > 0 && !audio.ended && !(mode === 'natural' && waiting)) {
    playing = true;
    audio.playbackRate = speed;
    audio.play().catch(() => {});
    changed();
  } else if (mode === 'natural') {
    playing = true;
    newGeneration();
    playPiece(true);
    changed();
  } else {
    play(index);
  }
}

export function pause() {
  playing = false;
  waiting = false;
  audio.pause();
  if (utterance) { utterance.onend = null; speechSynthesis.cancel(); utterance = null; }
  changed();
}

export function next() { if (index + 1 < items.length) play(index + 1); }
export function previous() { play(Math.max(0, index - 1)); }
export function nextSection() {
  const i = items.findIndex((x, j) => j > index && x.kind === 'heading');
  if (i >= 0) play(i);
}

function currentSection(): string {
  for (let i = Math.min(index, items.length - 1); i >= 0; i--) if (items[i].kind === 'heading') return items[i].pieces.join(' ');
  return '';
}

function setSpeed(s: number) {
  speed = s;
  localStorage.setItem('narrationSpeed', String(s));
  audio.playbackRate = s;
  if (mode === 'system' && playing) speakPiece();
  changed();
}

// ---- the natural voice ----

function newGeneration() {
  generation++;
  voiceApi.generation(generation).catch(() => {});
}

function clearClips() {
  for (const c of clips.values()) c.then((u) => { if (u) URL.revokeObjectURL(u); });
  clips.clear();
}

/** The pieces from (i, j) on, in reading order. */
function* piecesFrom(i: number, j: number): Generator<[number, number]> {
  for (let a = i; a < items.length; a++) for (let b = a === i ? j : 0; b < items[a].pieces.length; b++) yield [a, b];
}

/** Asks for the pieces ahead of what plays, one at a time (the voice makes one at a time anyway). */
async function produce() {
  if (producing || mode !== 'natural') return;
  producing = true;
  try {
    for (;;) {
      if (!playing || mode !== 'natural') break;
      let todo: [number, number] | null = null, ahead = 0;
      for (const [a, b] of piecesFrom(index, piece)) {
        if (ahead++ > AHEAD) break;
        if (!clips.has(`${a}:${b}`)) { todo = [a, b]; break; }
      }
      if (!todo) break;
      const [a, b] = todo;
      const it = items[a];
      const [before, after] = PAUSE[it.kind] || PAUSE.text;
      const last = b === it.pieces.length - 1;
      const gen = generation;
      const p = voiceApi.clip(it.pieces[b], naturalVoice, gen, b === 0 ? before : 0, last ? after : 0.08)
        .then((buf) => (buf.byteLength ? URL.createObjectURL(new Blob([buf], { type: 'audio/wav' })) : null))
        .catch((e) => { note = `The natural voice failed: ${e}`; changed(); return null; });
      clips.set(`${a}:${b}`, p);
      const url = await p;
      // not made (the generation moved on before its turn): forget it, so it is asked for again
      if (!url && gen !== generation && clips.get(`${a}:${b}`) === p) clips.delete(`${a}:${b}`);
      // let go of pieces already played
      for (const k of [...clips.keys()]) {
        const [x, y] = k.split(':').map(Number);
        if (x < index - 1 || (x === index && y < piece - 1)) { clips.get(k)!.then((u) => u && URL.revokeObjectURL(u)); clips.delete(k); }
      }
    }
  } finally {
    producing = false;
  }
}

/** Plays the current piece in the natural voice (made now if it isn't yet). */
async function playPiece(resume = false) {
  const gen = generation;
  const key = `${index}:${piece}`;
  if (!clips.has(key)) clearClips();     // a jump: what was made ahead is for elsewhere
  waiting = true;
  changed();
  for (let tries = 0; tries < 20; tries++) {
    produce();
    const p = clips.get(key);
    if (!p) { await new Promise((r) => setTimeout(r, 30)); if (gen !== generation || !playing) return; continue; }
    const url = await p;
    if (gen !== generation || !playing) return;
    if (!url && clips.get(key) !== p) continue;     // made for an older generation: asked for again
    waiting = false;
    if (!url) { advance(); return; }               // the voice failed on this piece: go on
    if (!resume || audio.src !== url) audio.src = url;
    audio.playbackRate = speed;
    audio.play().catch((e) => { note = `Couldn't play: ${e}`; changed(); });
    produce();
    changed();
    return;
  }
}

/** The next piece, or the next item. */
function advance() {
  if (piece + 1 < (items[index]?.pieces.length || 0)) {
    piece++;
  } else if (index + 1 < items.length) {
    index++;
    piece = 0;
    link?.highlight(items[index].id);
  } else {
    playing = false;
    changed();
    return;
  }
  if (mode === 'natural') playPiece(); else speakPiece();
  changed();
}

audio.addEventListener('ended', () => {
  if (!playing) return;
  if (mode === 'natural') advance();
  else if (mode === 'recorded') { playing = false; changed(); }
});
audio.addEventListener('timeupdate', () => {
  if (mode !== 'recorded' || !timings.length) return;
  const t = audio.currentTime;
  let lo = 0, hi = timings.length - 1;
  while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (timings[mid].start <= t + 0.05) lo = mid; else hi = mid - 1; }
  if (lo !== index) { index = lo; link?.highlight(items[lo].id); changed(); }
  else renderProgress();
});

// ---- the system's voice ----

function systemVoices(): Promise<SpeechSynthesisVoice[]> {
  if (!('speechSynthesis' in window)) return Promise.resolve([]);
  const english = () => speechSynthesis.getVoices().filter((v) => v.lang.toLowerCase().startsWith('en'));
  if (english().length) return Promise.resolve(english());
  // voices arrive a little after start
  return new Promise((resolve) => {
    const done = () => resolve(english());
    speechSynthesis.addEventListener('voiceschanged', done, { once: true });
    setTimeout(done, 1200);
  });
}

function speakPiece() {
  if (!('speechSynthesis' in window)) { playing = false; changed(); return; }
  const it = items[index];
  if (!it) return;
  if (utterance) { utterance.onend = null; speechSynthesis.cancel(); }
  const u = new SpeechSynthesisUtterance(it.pieces[piece] || '');
  const vs = speechSynthesis.getVoices();
  const v = vs.find((x) => x.voiceURI === systemVoice) || vs.find((x) => x.lang.toLowerCase().startsWith('en') && x.localService) || vs.find((x) => x.lang.toLowerCase().startsWith('en'));
  if (v) u.voice = v;
  u.rate = speed;
  u.onend = () => { if (utterance === u && playing) setTimeout(advance, it.kind === 'heading' && piece === it.pieces.length - 1 ? 350 : 60); };
  u.onerror = (e) => { if (utterance === u && e.error !== 'interrupted' && e.error !== 'canceled') { note = `The system voice failed: ${e.error}`; changed(); } };
  utterance = u;
  speechSynthesis.speak(u);
}

// ---- a narration made on the Mac ----

async function loadRecorded(id: string, pageItems: NarrationItem[]): Promise<boolean> {
  try {
    const json = await voiceApi.paperTimings(id);
    if (!json) return false;
    const t = JSON.parse(json) as Timings;
    if (!t.segments?.length) return false;
    const buf = await voiceApi.paperAudio(id);
    const text = new Map(pageItems.map((i) => [i.id, i.text]));
    items = t.segments.map((s) => ({ id: s.id, kind: s.kind, pieces: [text.get(s.id) || ''] }));
    timings = t.segments;
    audio.src = URL.createObjectURL(new Blob([buf], { type: 'audio/mp4' }));
    return true;
  } catch (e) {
    console.warn('narration', e);
    return false;
  }
}

// ---- the natural voice's download ----

onEvent<{ done: number; total: number }>('listen-download', (e) => { downloading = e.payload; renderBars(); renderVoiceDialog(); });

const mb = (n: number) => `${Math.round(n / 1e6)} MB`;

export async function downloadVoice() {
  if (downloading) return;
  downloading = { done: 0, total: (voiceStatus ?? await voiceApi.status()).download_bytes };
  changed(); renderVoiceDialog();
  try {
    voiceStatus = await voiceApi.download();
    status('The natural voice is ready');
    // switch the paper being read over to it, from the paragraph being read
    if (items.length && mode === 'system') {
      const was = playing;
      pause();
      mode = 'natural';
      note = '';
      voiceApi.warmUp(naturalVoice).catch(() => {});
      if (was) play(index);
    }
  } catch (e) {
    if (!String(e).includes('cancelled')) status(`Couldn't download the natural voice: ${e}`, 'error');
  } finally {
    downloading = null;
    changed(); renderVoiceDialog();
  }
}

/** Settings › Natural Voice…: download or delete it, pick the voice. */
export async function voiceDialog() {
  voiceStatus = await voiceApi.status().catch(() => null);
  if (!voices.length) voices = await voiceApi.voices().catch(() => []);
  document.querySelector('#voice-dialog')?.remove();
  const back = h('div.modal-back#voice-dialog', { onmousedown: (e: MouseEvent) => { if (e.target === back) back.remove(); } });
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') back.remove(); });
  document.body.append(back);
  renderVoiceDialog();
}

function renderVoiceDialog() {
  const back = document.querySelector('#voice-dialog') as HTMLElement | null;
  if (!back) return;
  const s = voiceStatus;
  const select = h('select', { onchange: (e: Event) => setNaturalVoice((e.target as HTMLSelectElement).value) },
    voices.map((v) => h('option', { value: v.id, text: v.name, selected: v.id === naturalVoice }))) as HTMLSelectElement;
  const body = downloading
    ? [h('p', { text: `Downloading the natural voice… ${mb(downloading.done)} of ${mb(downloading.total)}` }),
       h('progress', { max: downloading.total || 1, value: downloading.done }),
       h('div.buttons', {}, h('button', { text: 'Cancel', onclick: () => voiceApi.cancel() }))]
    : s?.installed
      ? [h('p', { text: `The natural voice is on this computer (${mb(s.disk_bytes)}). It runs here; nothing you listen to leaves the computer.` }),
         h('label.listen-voice', {}, 'Voice ', select),
         h('div.buttons', {},
           h('button.danger', { text: 'Delete Natural Voice', onclick: async () => {
             if (mode === 'natural' && items.length) stop();
             voiceStatus = await voiceApi.remove().catch((e) => { status(String(e), 'error'); return voiceStatus; });
             renderVoiceDialog();
           } }),
           h('button.primary', { text: 'Done', onclick: () => back.remove() }))]
      : [h('p', { text: `Lunta can read papers aloud in a natural voice (Kokoro, an open speech model) that runs on this computer. It is a one-time download of ${mb(s?.download_bytes || 0)} from the sherpa-onnx project on GitHub; until then Listen uses the system's voice.` }),
         h('div.buttons', {}, h('button', { text: 'Not Now', onclick: () => back.remove() }),
           h('button.primary', { text: `Download (${mb(s?.download_bytes || 0)})`, onclick: () => downloadVoice() }))];
  back.replaceChildren(h('div.modal', {}, h('h3', { text: 'Natural Voice' }), ...body));
}

function setNaturalVoice(id: string) {
  naturalVoice = id;
  localStorage.setItem('neuralVoice', id);
  if (mode === 'natural' && items.length) {
    clearClips();
    voiceApi.warmUp(id).catch(() => {});
    if (playing) play(index);
  }
  changed();
}

// ---- the player bar (in the reader) and Now Playing (in the library) ----

function voiceMenu(x: number, y: number) {
  const list: MenuItem[] = SPEEDS.map((s) => ({ label: `${s}×`, checked: s === speed, action: () => setSpeed(s) }));
  list.push('-');
  if (mode === 'natural') {
    for (const v of voices) list.push({ label: v.name, checked: v.id === naturalVoice, action: () => setNaturalVoice(v.id) });
  } else if (mode === 'system') {
    for (const v of speechSynthesis.getVoices().filter((v) => v.lang.toLowerCase().startsWith('en')).slice(0, 25)) {
      list.push({ label: `${v.name} · ${v.lang}`, checked: v.voiceURI === systemVoice, action: () => {
        systemVoice = v.voiceURI; localStorage.setItem('narrationVoice', systemVoice); if (playing) speakPiece(); changed();
      } });
    }
    list.push('-', { label: 'Natural Voice…', action: () => voiceDialog() });
  }
  menu(x, y, list);
}

function progress(): number {
  if (mode === 'recorded' && audio.duration > 0) return audio.currentTime / audio.duration;
  return items.length ? index / items.length : 0;
}

function renderProgress() {
  document.querySelectorAll<HTMLProgressElement>('.listen-progress').forEach((p) => { p.value = progress(); });
}

function controls(): HTMLElement {
  return h('div.listen-controls', {},
    h('button.icon', { text: '⏮', title: 'Previous paragraph', onclick: previous }),
    h('button.icon.listen-play', { text: playing ? '⏸' : '▶', title: playing ? 'Pause' : 'Play', onclick: toggle }),
    h('button.icon', { text: '⏭', title: 'Next paragraph', onclick: next }),
    h('button.icon', { text: '⏭︎|', title: 'Next section', onclick: nextSection }));
}

function renderBars() {
  // the reader's own player
  const bar = document.querySelector('#listen-bar') as HTMLElement | null;
  if (bar) {
    const show = items.length > 0 && link !== null;
    bar.hidden = !show;
    if (show) {
      const natural = mode !== 'system';
      let line: HTMLElement | null = null;
      if (downloading) line = h('div.listen-note', { text: `Downloading the natural voice… ${mb(downloading.done)} of ${mb(downloading.total)}` });
      else if (note === 'offer' || note === 'novoice') line = h('div.listen-note', {}, note === 'offer' ? 'Using the system voice. ' : 'This computer has no voice to read with. ',
        h('button.link', { text: `Download the natural voice (${mb(voiceStatus?.download_bytes || 0)})`, onclick: () => voiceDialog() }),
        note === 'offer' ? ' for much more natural speech.' : ' to listen.');
      else if (note) line = h('div.listen-note', { text: note });
      bar.replaceChildren(
        h('progress.listen-progress', { max: 1, value: progress() }),
        h('div.listen-row', {},
          h('div.listen-info', {},
            h('div.listen-section', { text: currentSection() || 'Listening' }),
            h('div.listen-sub', {},
              natural ? h('span.listen-pill', { text: '〰 Natural voice' }) : null,
              waiting && playing ? h('span.listen-spin') : null,
              h('span', { text: title }))),
          controls(),
          h('button.listen-speed', { text: `${speed}×`, title: 'Speed and voice', onclick: (e: MouseEvent) => {
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            voiceMenu(r.left, r.top);
            const m = $('#menu');     // above the button: the bar is at the bottom of the window
            if (m) m.style.top = `${Math.max(4, r.top - m.offsetHeight - 6)}px`;
          } }),
          h('button.icon', { text: '✕', title: 'Stop listening', onclick: stop })),
        ...(line ? [line] : []));
    }
  }
  // Now Playing, wherever the reader's player isn't
  let np = document.querySelector('#now-playing') as HTMLElement | null;
  const showNp = items.length > 0 && link === null;
  if (!showNp) { np?.remove(); return; }
  if (!np) { np = h('div#now-playing'); document.body.append(np); }
  np.replaceChildren(
    h('button.np-open', { title: 'Open the paper at the paragraph being read', onclick: () => openPaper?.(paperId) },
      h('span.np-wave' + (playing ? '.on' : ''), { text: '〰' }),
      h('span.np-text', {}, h('span.np-title', { text: title || 'Listening' }), h('span.np-sub', { text: currentSection() || (mode === 'system' ? 'System voice' : 'Natural voice') }))),
    controls(),
    h('button.icon', { text: '✕', title: 'Stop', onclick: stop }));
}

/** The reader's player bar (placed by reader.ts). */
export function playerBar(): HTMLElement {
  const bar = h('div#listen-bar', { hidden: true });
  queueMicrotask(renderBars);
  return bar;
}

// ---- media keys and the system's media controls (Windows' media flyout; MPRIS where WebKitGTK has it) ----

function updateMediaSession() {
  const ms = (navigator as Navigator & { mediaSession?: MediaSession }).mediaSession;
  if (!ms) return;
  if (!items.length) { ms.metadata = null; ms.playbackState = 'none'; return; }
  ms.metadata = new MediaMetadata({ title, artist: currentSection() || 'Lunta', album: 'Lunta' });
  ms.playbackState = playing ? 'playing' : 'paused';
}

(() => {
  const ms = (navigator as Navigator & { mediaSession?: MediaSession }).mediaSession;
  if (!ms) return;
  const set = (a: MediaSessionAction, f: () => void) => { try { ms.setActionHandler(a, f); } catch { /* not supported */ } };
  set('play', toggle);
  set('pause', pause);
  set('nexttrack', next);
  set('previoustrack', previous);
  set('stop', stop);
})();

// keep the library's bar out of the way of the status line
export function nowPlayingShown() { return !!$('#now-playing'); }
