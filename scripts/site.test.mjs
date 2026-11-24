import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const ROOT = path.resolve(import.meta.dirname, "..");
const SITE = path.join(ROOT, "site");
const indexHtml = fs.readFileSync(path.join(SITE, "index.html"), "utf8");
const downloadHtml = fs.readFileSync(path.join(SITE, "download.html"), "utf8");
const gettingStartedHtml = fs.readFileSync(path.join(SITE, "getting-started.html"), "utf8");
const styleCss = fs.readFileSync(path.join(SITE, "style.css"), "utf8");
const siteJs = fs.readFileSync(path.join(SITE, "js", "site.js"), "utf8");

test("site HTML pages make zero third-party asset requests (privacy guarantee)", () => {
  const externalAssetRe = /<(?:script|img|audio|video|source)\b[^>]*\bsrc=["']https?:\/\//gi;
  const externalStyleRe = /<link\b[^>]*rel=["'](?:stylesheet|preload)["'][^>]*href=["']https?:\/\//gi;
  for (const [name, html] of [["index.html", indexHtml], ["download.html", downloadHtml], ["getting-started.html", gettingStartedHtml]]) {
    assert.deepEqual(html.match(externalAssetRe) || [], [], `Found external script/media src in ${name}`);
    assert.deepEqual(html.match(externalStyleRe) || [], [], `Found external stylesheet/preload href in ${name}`);
  }
});

test("all local JS, CSS, font, audio, and image assets referenced in site/index.html exist", () => {
  const attrRe = /\b(?:src|href)=["']([^"'#?]+)["']/g;
  const checked = new Set();
  let m;
  while ((m = attrRe.exec(indexHtml)) !== null) {
    const ref = m[1];
    if (/^(?:https?:|mailto:|tel:|data:)/i.test(ref)) continue;
    if (ref === "./" || ref.endsWith(".html")) continue;
    if (checked.has(ref)) continue;
    checked.add(ref);
    const full = path.join(SITE, ref);
    assert.ok(fs.existsSync(full), `Missing local asset referenced in index.html: ${ref}`);
  }
  assert.ok(checked.has("js/gsap.min.js"), "Self-hosted GSAP must be referenced");
  assert.ok(checked.has("js/ScrollTrigger.min.js"), "Self-hosted ScrollTrigger must be referenced");
  assert.ok(checked.has("js/site.js"), "site/js/site.js must be referenced");
});

test("self-hosted scripts have valid JavaScript syntax", () => {
  for (const file of ["gsap.min.js", "ScrollTrigger.min.js", "site.js"]) {
    execFileSync(process.execPath, ["--check", path.join(SITE, "js", file)]);
  }
});

test("all interactive demo DOM hooks in site/js/site.js exist in site/index.html", () => {
  const requiredHooks = [
    "read-progress",
    "hero-canvas",
    "data-cta-ios",
    "data-cta-desktop",
    "data-showcase-phone",
    "data-read-theme",
    "data-cite-ref",
    "data-cite-popover",
    "data-fig-open",
    "data-fig-overlay",
    "data-count-to",
    "data-hl-demo",
    "data-hl-color",
    "data-hl-note-btn",
    "data-hl-note",
    "data-cite-demo",
    "data-cite-style",
    "data-cite-page",
    "data-listen",
    "data-agent-demo",
    "data-mcp-tool",
    "data-agent-output",
    "data-copy-mcp",
    "data-import-count",
    "data-import-file",
    "data-ai-demo",
    "data-ai-q",
    "data-switch-demo",
    "data-source-tab",
    "data-theme-toggle",
    "compare-body"
  ];
  for (const hook of requiredHooks) {
    assert.ok(indexHtml.includes(hook), `index.html is missing required interactive hook: ${hook}`);
  }

  for (const [name, html] of [["download.html", downloadHtml], ["getting-started.html", gettingStartedHtml]]) {
    assert.ok(html.includes('src="js/site.js"'), `${name} should load self-hosted js/site.js`);
    assert.ok(html.includes("read-progress"), `${name} should include reading progress bar`);
    assert.ok(html.includes("data-theme-toggle"), `${name} should include header theme toggle`);
  }
  assert.ok(downloadHtml.includes('data-os="mac"'), "download.html should tag OS rows for auto-highlight");

  for (const style of ["nature", "apa", "vancouver", "cell", "ieee", "bibtex"]) {
    assert.ok(indexHtml.includes(`data-cite-style="${style}"`), `Missing citation style button: ${style}`);
    assert.ok(siteJs.includes(`${style}:`), `Missing citation formatter entry in site.js: ${style}`);
  }

  for (const theme of ["light", "paper", "quiet", "black"]) {
    assert.ok(indexHtml.includes(`data-read-theme="${theme}"`), `Missing reading theme button: ${theme}`);
    assert.ok(styleCss.includes(`[data-theme="${theme}"]`), `Missing reading theme CSS: ${theme}`);
  }
});

test("CSS scroll-timeline fallback rules are guarded against conflicting with GSAP", () => {
  const timelineBlocks = styleCss.match(/@supports\s*\(animation-timeline:[^\{]+\{[\s\S]*?\n  \}/g) || [];
  assert.ok(timelineBlocks.length >= 4, `Expected >=4 CSS scroll-timeline fallback blocks, got ${timelineBlocks.length}`);
  for (const block of timelineBlocks) {
    assert.ok(block.includes("html:not(.has-gsap)"), `Unguarded animation-timeline block:\n${block}`);
  }
});
