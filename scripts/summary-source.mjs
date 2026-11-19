#!/usr/bin/env node
// Prints what the reader's Summary card is made from (luntaSummarySource in
// apple/Shared/SummaryCard.swift) for an EPUB made by the converter, to try the on-device model on
// it outside the apps:
//
//   node scripts/summary-source.mjs book.epub [words] > source.txt
//   cargo run --release --features llm --example ai -- summarize <model.gguf> source.txt "<title>"
//
// The chapters are read in spine order (with `unzip`) and joined, as the apps' continuous
// reading view joins them; the page is the tests' small DOM (scripts/reader-tests/mini-dom.mjs).

import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join, posix } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, run } from './reader-tests/mini-dom.mjs';

const [epub, words = '1900'] = process.argv.slice(2);
if (!epub) { console.error('usage: node scripts/summary-source.mjs book.epub [words]'); process.exit(2); }
const unzip = (name) => execFileSync('unzip', ['-p', epub, name], { maxBuffer: 1 << 28 }).toString('utf8');

const container = unzip('META-INF/container.xml');
const opfPath = container.match(/full-path="([^"]+)"/)[1];
const opf = unzip(opfPath);
const items = new Map([...opf.matchAll(/<item\b[^>]*>/g)].map((m) => [m[0].match(/id="([^"]+)"/)?.[1], m[0].match(/href="([^"]+)"/)?.[1]]));
const spine = [...opf.matchAll(/<itemref\b[^>]*idref="([^"]+)"/g)].map((m) => items.get(m[1])).filter(Boolean);
const bodies = spine.map((href) => {
  const x = unzip(posix.join(posix.dirname(opfPath), href));
  return (x.match(/<body[^>]*>([\s\S]*)<\/body>/i) || [, ''])[1];
});
const doc = parse(`<html><head></head><body>${bodies.join('\n')}</body></html>`);

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');
const src = readFileSync(join(repo, 'apple/Shared/SummaryCard.swift'), 'utf8').replace(/\r\n/g, '\n');
const m = src.match(/static let summaryJS = #"""\n([\s\S]*?)\n([ \t]*)"""#/);
const script = m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n');
const win = run(script, doc);
const out = JSON.parse(win.luntaSummarySource(Number(words), ''));
console.error(`${out.words} words: ${out.parts.join(', ')}`);
process.stdout.write(out.text + '\n');
