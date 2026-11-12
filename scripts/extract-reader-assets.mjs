#!/usr/bin/env node
// Copies the reading view's CSS and scripts out of the Swift sources (where the Mac and iPhone
// apps keep them as string literals) into desktop/src/reader/generated/, so the desktop app
// renders papers with exactly the same reader code. Run it whenever the Swift assets change:
//
//   node scripts/extract-reader-assets.mjs          (also runs before `npm run dev` / `build`)
//   node scripts/extract-reader-assets.mjs --check  (exit 1 if the generated files are stale)
//
// Only plain `static let name = """ … """` / `#""" … """#` literals are read; anything the
// script doesn't understand (string interpolation, an unexpected post-processing call) stops it
// with an error rather than producing a silently different asset.

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(repo, 'desktop', 'src', 'reader', 'generated');
const check = process.argv.includes('--check');

/** Reads `static let <name> = """…"""` (or the raw `#"""…"""#` form) from a Swift file. */
function literal(file, name) {
  const src = readFileSync(join(repo, file), 'utf8').replace(/\r\n/g, '\n');   // Windows checkouts
  const re = new RegExp(`static let ${name} = (#?)"""\\n([\\s\\S]*?)\\n([ \\t]*)"""\\1([^\\n]*)`);
  const m = src.match(re);
  if (!m) throw new Error(`${file}: no multi-line string literal "${name}"`);
  const [, hash, body, indent, rest] = m;
  // Swift strips the closing delimiter's indentation from every line
  const lines = body.split('\n').map((l, i) => {
    if (l.trim() === '') return '';
    if (!l.startsWith(indent)) throw new Error(`${file}: ${name}: line ${i + 1} is indented less than the closing """`);
    return l.slice(indent.length);
  });
  let text = lines.join('\n');
  text = hash ? rawEscapes(text, file, name) : escapes(text, file, name);
  return { text, rest: rest.trim(), file };
}

function rawEscapes(t, file, name) {
  if (t.includes('\\#(')) throw new Error(`${file}: ${name}: string interpolation is not supported`);
  if (/\\#[^(]/.test(t)) throw new Error(`${file}: ${name}: raw-string escapes are not supported`);
  return t;
}

function escapes(t, file, name) {
  return t.replace(/\\(\n|u\{([0-9a-fA-F]+)\}|.)/g, (all, c, hex) => {
    if (hex) return String.fromCodePoint(parseInt(hex, 16));
    switch (c) {
      case '\n': return '';          // line continuation
      case 'n': return '\n';
      case 't': return '\t';
      case 'r': return '\r';
      case '0': return '\0';
      case '\\': case '"': case "'": return c;
      case '(': throw new Error(`${file}: ${name}: string interpolation is not supported`);
      default: throw new Error(`${file}: ${name}: unknown escape \\${c}`);
    }
  });
}

function plain(file, name) {
  const l = literal(file, name);
  if (l.rest) throw new Error(`${file}: ${name}: unexpected code after the literal: ${l.rest}`);
  return l.text;
}

const cloud = 'apple/Shared/CloudLibrary.swift';
const css = plain(cloud, 'css');
const figureJS = plain(cloud, 'figureJS');
const overflowJS = plain(cloud, 'overflowJS');
const trackingJS = plain(cloud, 'trackingJS');
const narrationJS = plain(cloud, 'narrationJS');
const pagedJS = plain('apple/Shared/PageTurn.swift', 'pagedJS');
const positionJS = plain('apple/Shared/ReadingPosition.swift', 'positionJS');
const annotationCSS = plain('apple/Shared/Highlights.swift', 'annotationCSS');

// annotationJS is a template: Swift fills in `CSS` with annotationCSS (backticks removed)
const ann = literal('apple/Shared/Highlights.swift', 'annotationJS');
const expected = '.replacingOccurrences(of: "CSS", with: annotationCSS.replacingOccurrences(of: "`", with: ""))';
if (ann.rest !== expected) throw new Error(`Highlights.swift: annotationJS is post-processed differently now: ${ann.rest}`);
const annotationJS = ann.text.split('CSS').join(annotationCSS.split('`').join(''));

// styleScript (CloudLibrary.swift) puts `css` into a <style> on the root element, then runs the
// figure and overflow scripts; the same here. (It also adds a mobile viewport tag: kept, harmless.)
const cloudSrc = readFileSync(join(repo, cloud), 'utf8').replace(/\r\n/g, '\n');
for (const piece of ["s.textContent=`\\(c)`;document.documentElement.appendChild(s);", '+ figureJS + overflowJS']) {
  if (!cloudSrc.includes(piece)) throw new Error(`${cloud}: styleScript changed; update extract-reader-assets.mjs`);
}
const styleJS =
  `var s=document.createElement('style');s.textContent=${JSON.stringify(css.split('`').join(''))};document.documentElement.appendChild(s);` +
  "var m=document.querySelector('meta[name=viewport]');if(!m){m=document.createElement('meta');m.name='viewport';(document.head||document.documentElement).appendChild(m);}m.content='width=device-width, initial-scale=1, minimum-scale=1, maximum-scale=1, user-scalable=no, viewport-fit=cover';" +
  figureJS + overflowJS;

const header = (from) => `/* Generated by scripts/extract-reader-assets.mjs from ${from}. Do not edit. */\n`;
// In the order the apps inject them (EpubWebView in apple/Mac/ReaderWindow.swift).
const files = {
  'style.js': [header(`${cloud} (ReaderAssets.styleScript)`), styleJS],
  'tracking.js': [header(`${cloud} (ReaderAssets.trackingJS)`), trackingJS],
  'narration.js': [header(`${cloud} (ReaderAssets.narrationJS)`), narrationJS],
  'annotation.js': [header('apple/Shared/Highlights.swift (ReaderAssets.annotationJS)'), annotationJS],
  'position.js': [header('apple/Shared/ReadingPosition.swift (ReaderAssets.positionJS)'), positionJS],
  'paged.js': [header('apple/Shared/PageTurn.swift (ReaderAssets.pagedJS)'), pagedJS],
  'reader.css': [header(`${cloud} (ReaderAssets.css)`), css],
};

mkdirSync(out, { recursive: true });
let stale = [];
for (const [name, [h, body]] of Object.entries(files)) {
  const path = join(out, name);
  const text = h + body + '\n';
  const old = existsSync(path) ? readFileSync(path, 'utf8').replace(/\r\n/g, '\n') : null;
  if (old === text) continue;
  stale.push(relative(repo, path));
  if (!check) writeFileSync(path, text);
}
if (check && stale.length) {
  console.error(`Reader assets out of date (run node scripts/extract-reader-assets.mjs):\n  ${stale.join('\n  ')}`);
  process.exit(1);
}
console.log(stale.length ? `Updated ${stale.join(', ')}` : 'Reader assets up to date');
