import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const ROOT = path.resolve(import.meta.dirname, "..");
const SITE = path.join(ROOT, "site");
const indexHtml = fs.readFileSync(path.join(SITE, "index.html"), "utf8");
const featuresHtml = fs.readFileSync(path.join(SITE, "features.html"), "utf8");
const downloadHtml = fs.readFileSync(path.join(SITE, "download.html"), "utf8");
const gettingStartedHtml = fs.readFileSync(path.join(SITE, "getting-started.html"), "utf8");
const styleCss = fs.readFileSync(path.join(SITE, "style.css"), "utf8");
const siteJs = fs.readFileSync(path.join(SITE, "js", "site.js"), "utf8");

test("site HTML pages make zero third-party asset requests (privacy guarantee)", () => {
  const externalAssetRe = /<(?:script|img|audio|video|source)\b[^>]*\bsrc=["']https?:\/\//gi;
  const externalStyleRe = /<link\b[^>]*rel=["'](?:stylesheet|preload)["'][^>]*href=["']https?:\/\//gi;
  for (const [name, html] of [["index.html", indexHtml], ["features.html", featuresHtml], ["download.html", downloadHtml], ["getting-started.html", gettingStartedHtml]]) {
    assert.deepEqual(html.match(externalAssetRe) || [], [], `Found external script/media src in ${name}`);
    assert.deepEqual(html.match(externalStyleRe) || [], [], `Found external stylesheet/preload href in ${name}`);
  }
});

test("all local JS, CSS, font, audio, and image assets referenced in index.html and features.html exist", () => {
  for (const [name, html] of [["index.html", indexHtml], ["features.html", featuresHtml]]) {
    const attrRe = /\b(?:src|href)=["']([^"'#?]+)["']/g;
    const checked = new Set();
    let m;
    while ((m = attrRe.exec(html)) !== null) {
      const ref = m[1];
      if (/^(?:https?:|mailto:|tel:|data:)/i.test(ref)) continue;
      if (ref === "./") continue;
      if (checked.has(ref)) continue;
      checked.add(ref);
      const full = path.join(SITE, ref);
      assert.ok(fs.existsSync(full), `Missing local file referenced in ${name}: ${ref}`);
    }
    assert.ok(checked.has("js/gsap.min.js"), `${name}: self-hosted GSAP must be referenced`);
    assert.ok(checked.has("js/ScrollTrigger.min.js"), `${name}: self-hosted ScrollTrigger must be referenced`);
    assert.ok(checked.has("js/site.js"), `${name}: site/js/site.js must be referenced`);
  }
});

test("self-hosted scripts have valid JavaScript syntax", () => {
  for (const file of ["gsap.min.js", "ScrollTrigger.min.js", "site.js"]) {
    execFileSync(process.execPath, ["--check", path.join(SITE, "js", file)]);
  }
});

test("the interactive demo DOM hooks in site/js/site.js exist on the page that shows each demo", () => {
  const indexHooks = [
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
    "data-theme-toggle"
  ];
  for (const hook of indexHooks) {
    assert.ok(indexHtml.includes(hook), `index.html is missing required interactive hook: ${hook}`);
  }
  const featuresHooks = [
    "read-progress",
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
  for (const hook of featuresHooks) {
    assert.ok(featuresHtml.includes(hook), `features.html is missing required interactive hook: ${hook}`);
  }

  for (const [name, html] of [["download.html", downloadHtml], ["getting-started.html", gettingStartedHtml]]) {
    assert.ok(html.includes('src="js/site.js"'), `${name} should load self-hosted js/site.js`);
    assert.ok(html.includes("read-progress"), `${name} should include reading progress bar`);
    assert.ok(html.includes("data-theme-toggle"), `${name} should include header theme toggle`);
  }
  assert.ok(downloadHtml.includes('data-os="mac"'), "download.html should tag OS rows for auto-highlight");

  for (const style of ["nature", "apa", "vancouver", "cell", "ieee", "bibtex"]) {
    assert.ok(indexHtml.includes(`data-cite-style="${style}"`), `Missing citation style button: ${style}`);
    assert.ok(featuresHtml.includes(`data-cite-style="${style}"`), `Missing citation style button on features.html: ${style}`);
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

const ids = (html) => [...html.matchAll(/\bid="([^"]+)"/g)].map((m) => m[1]);
const cards = (html) => [...html.matchAll(/<article class="cell[^"]*"[^>]*\bid="([^"]+)"/g)].map((m) => m[1]);

test("the front page is short: four feature cards, one number, a row of platforms, no comparison table", () => {
  assert.deepEqual(cards(indexHtml), ["read", "highlight", "new-for-you", "cite"]);
  assert.equal((indexHtml.match(/data-count-to=/g) || []).length, 1, "index.html should show one benchmark number");
  assert.ok(!indexHtml.includes('class="platforms"'), "the comparison table belongs on features.html");
  assert.ok(!indexHtml.includes('id="switch"'), "Switching from Zotero belongs on features.html");
  assert.ok(!indexHtml.includes('id="open-source"'), "the open source block belongs on features.html");
  assert.ok(indexHtml.includes('class="plat-row"'), "index.html should have the compact platform row");
  assert.ok(indexHtml.includes('href="features.html">All features'), "index.html should link to All features");
  assert.ok(indexHtml.includes('href="app-privacy.html"'), "index.html should link to the privacy policy");
});

test("features.html has every feature card, the four numbers, Switching from Zotero, the table and open source", () => {
  assert.deepEqual(cards(featuresHtml), ["read", "highlight", "ai", "cite", "listen", "library", "new-for-you", "extension", "agents"]);
  assert.equal((featuresHtml.match(/data-count-to=/g) || []).length, 4, "features.html should show all four benchmark numbers");
  for (const id of ["features", "numbers", "switch", "platforms", "compare", "privacy", "open-source"]) {
    assert.ok(ids(featuresHtml).includes(id), `features.html is missing #${id}`);
  }
  assert.ok(featuresHtml.includes('<table class="platforms">'), "features.html should have the comparison table");
});

test("no duplicate ids, and every #anchor link on index.html and features.html points at an element that exists", () => {
  const pages = { "index.html": indexHtml, "features.html": featuresHtml };
  for (const [name, html] of Object.entries(pages)) {
    const list = ids(html);
    const dupes = list.filter((id, i) => list.indexOf(id) !== i);
    assert.deepEqual(dupes, [], `${name} has duplicate ids`);
  }
  for (const [name, html] of Object.entries(pages)) {
    for (const m of html.matchAll(/\bhref="((?:index\.html|features\.html|\.\/)?)#([^"]+)"/g)) {
      const target = m[1] === "features.html" ? "features.html" : m[1] === "" ? name : "index.html";
      assert.ok(ids(pages[target]).includes(m[2]), `${name}: link ${m[0]} has no target in ${target}`);
    }
  }
});

test("every page's header links Features to features.html, and the sitemap lists it", () => {
  // picker.html is Google's file picker for the extension, opened by it alone: no site header or footer.
  for (const file of fs.readdirSync(SITE).filter((f) => f.endsWith(".html") && f !== "picker.html")) {
    const html = fs.readFileSync(path.join(SITE, file), "utf8");
    assert.ok(/<a class="wide" href="features\.html"[^>]*>Features<\/a>/.test(html), `${file}: header Features should link to features.html`);
    assert.ok(!html.includes('href="./#features"') && !html.includes('href="#features"'), `${file}: stale link to the front page's features`);
  }
  assert.ok(fs.readFileSync(path.join(SITE, "sitemap.xml"), "utf8").includes("https://getlunta.app/features.html"));
});
