import Foundation

// The Summary card at the top of the reading view (Summary.swift makes the summary): the page's
// side. luntaSummarySource picks what the summary is made from (the abstract, the aims at the end
// of the introduction, the start of the discussion and conclusion), and luntaSummaryShow draws the
// card under the title. Like the figure cards, the card's words are drawn from attributes (CSS
// content), so it adds no text to the page: highlights, find, reading places and Listen, which all
// count the page's text, never see it, and it can't be selected, copied or cited.

extension ReaderAssets {
    static let summaryJS = #"""
    (function(){
      if (window.luntaSummarySource) return;
      var st = document.createElement('style');
      st.textContent =
        '.m-summary { position: relative; margin: 1.5em 0 1.7em; padding: .8em 1.05em .75em; border-radius: 12px; background: #f2f1ee; border: 1px solid rgba(127,127,127,.16);' +
        ' -webkit-user-select: none; user-select: none; -webkit-touch-callout: none; break-inside: avoid; -webkit-column-break-inside: avoid; font-size: 15px; line-height: 1.45; text-align: left; }' +
        '.m-sum-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: .3em; }' +
        '.m-sum-title { font: 600 13px -apple-system, "Helvetica Neue", sans-serif; letter-spacing: .01em; opacity: .7; }' +
        // the summary in the system font at a fixed, compact size: in the reader's own (large on a phone)
        // four sentences filled more than a page and the card broke across pages
        '.m-sum-text { display: block; font: 15px/1.45 -apple-system, "Helvetica Neue", sans-serif; }' +
        '.m-sum-title::before, .m-sum-text::before, .m-sum-note::before, .m-sum-item::before { content: attr(data-t); }' +
        '.m-sum-more { -webkit-appearance: none; appearance: none; border: 0; background: none; margin: -6px -8px -6px 0; padding: 4px 8px; border-radius: 7px;' +
        ' cursor: pointer; color: inherit; opacity: .55; font: 600 1rem -apple-system, sans-serif; line-height: 1; -webkit-tap-highlight-color: transparent; }' +
        '.m-sum-more::before { content: "⋯"; }' +
        '.m-sum-more:hover, .m-summary.m-sum-open .m-sum-more { opacity: .9; background: rgba(127,127,127,.14); }' +
        '.m-sum-loading .m-sum-text { opacity: .5; animation: m-sum-pulse 1.6s ease-in-out infinite; }' +
        '@keyframes m-sum-pulse { 50% { opacity: .25; } }' +
        '.m-sum-note { margin-top: .5em; font: 11px -apple-system, "Helvetica Neue", sans-serif; opacity: .5; }' +
        '.m-sum-note:not([data-t]), .m-sum-note[data-t=""] { display: none; }' +
        '.m-sum-pop { display: none; position: absolute; right: .5em; top: 2.3em; z-index: 30; min-width: 11em; padding: 4px; border-radius: 10px; background: #fff;' +
        ' box-shadow: 0 8px 28px rgba(0,0,0,.18), 0 0 0 1px rgba(0,0,0,.07); }' +
        '.m-summary.m-sum-open .m-sum-pop { display: block; }' +
        '.m-sum-item { display: block; width: 100%; -webkit-appearance: none; appearance: none; border: 0; background: none; text-align: left; padding: 6px 10px; border-radius: 6px;' +
        ' font: .85rem -apple-system, "Helvetica Neue", sans-serif; color: #1d1d1f; cursor: pointer; -webkit-tap-highlight-color: transparent; }' +
        '.m-sum-item:hover { background: rgba(127,127,127,.14); }' +
        '@media (prefers-color-scheme: dark) { .m-summary { background: #151515; border-color: #2b2b2b; } .m-sum-pop { background: #2c2c2e; box-shadow: 0 8px 28px rgba(0,0,0,.5), 0 0 0 1px #3a3a3c; } .m-sum-item { color: #e8e6e1; } }' +
        'html[data-theme="light"] .m-summary { background: #f3f3f5; border-color: #e2e2e6; }' +
        'html[data-theme="paper"] .m-summary { background: #efe4cc; border-color: #d9c9a8; } html[data-theme="paper"] .m-sum-pop { background: #faf4e6; }' +
        'html[data-theme="paper"] .m-sum-item { color: #3b3025; }' +
        'html[data-theme="quiet"] .m-summary { background: #33322f; border-color: #45433f; } html[data-theme="quiet"] .m-sum-pop { background: #3b3a37; }' +
        'html[data-theme="quiet"] .m-sum-item { color: #d6d1c6; }';
      document.documentElement.appendChild(st);

      // ---- what the summary is made from ----
      // not part of the paper's running text
      var SKIP = 'figure, table, .table-wrap, ol.refs, .refs, .references, .notes, .titlepage, .frontmatter, .affiliations, .license, .doi, .journal, nav, .supp, aside, .fn, .m-summary';
      function squash(s){ return (s || '').replace(/\s+/g, ' ').trim(); }
      function count(s){ return s ? s.split(' ').length : 0; }
      // a section heading's kind, from its words ("2. Results and Discussion" -> results)
      function kindOf(title){
        var t = squash(title).toLowerCase().replace(/^(\d+(\.\d+)*\.?|[ivxlc]+[.)]|[a-z][.)])\s+/, '');
        if (/^graphical abstract/.test(t)) return 'other';
        if (/^results? (and|&) discussions?\b/.test(t)) return 'results';
        if (/^((general|summary (and|&)) )?discussions?\b/.test(t)) return 'discussion';
        if (/^((summary (and|&)) )?(conclusions?|concluding)\b(?! validity)/.test(t)) return 'conclusion';
        if (/^(abstract|summary|structured abstract)\b/.test(t)) return 'abstract';
        if (/^(introduction|background)\b/.test(t)) return 'intro';
        if (/^results?\b/.test(t)) return 'results';
        if (/^(references|bibliography|literature cited|acknowledge?ments?|funding|author contributions?|contributions|data (and code )?availability|code availability|declarations?|competing|conflicts? of interest|ethics|supplementary|supporting information|notes|footnotes|abbreviations|disclosures?|additional information|appendix|appendices)\b/.test(t)) return 'back';
        if (/^(methods|materials and methods|methodology|experimental|online methods)\b/.test(t)) return 'methods';
        return '';
      }
      // the paper's paragraphs in order, each with the kind of section it is in (the innermost
      // heading that has a kind) and which section that is
      // a PDF's footnotes that ended up in the text
      var JUNK = /(creativecommons\.org|open access article|declaration of competing|competing interests?:|^supplementary (data|material)|^correspond|[\w.]+@[\w-]+\.\w)/i;
      function paragraphs(){
        var out = [], stack = [], sec = 0;
        var els = document.querySelectorAll('h1, h2, h3, h4, h5, h6, p, li');
        for (var i = 0; i < els.length; i++) {
          var el = els[i], tag = el.localName || el.tagName.toLowerCase();
          if (el.closest(SKIP)) continue;
          if (/^h\d$/.test(tag)) {
            var lv = +tag.charAt(1);
            while (stack.length && stack[stack.length - 1].level >= lv) stack.pop();
            stack.push({ level: lv, kind: kindOf(el.textContent), sec: ++sec });
            continue;
          }
          if (tag === 'li' && el.querySelector('p')) continue;
          var t = squash(el.textContent);
          if (count(t) < 6 || /^(key ?words|abbreviations)\b/i.test(t) || JUNK.test(t)) continue;
          // (a structured abstract's Background … Conclusions are the abstract; anything in an appendix is back matter)
          var kind = 'other', at = 0, k;
          for (k = 0; k < stack.length; k++) if (stack[k].kind === 'abstract' || stack[k].kind === 'back') { kind = stack[k].kind; at = stack[k].sec; break; }
          if (k === stack.length) for (k = stack.length - 1; k >= 0; k--) if (stack[k].kind) { kind = stack[k].kind; at = stack[k].sec; break; }
          out.push({ kind: kind, sec: at, text: t, item: !!el.closest('li') });
        }
        return out;
      }
      // the first n words, ending at a sentence end when one is near
      function clip(t, n){
        var w = t.split(' ');
        if (w.length <= n) return t;
        var s = w.slice(0, n).join(' '), end = Math.max(s.lastIndexOf('. '), s.lastIndexOf('? '), s.lastIndexOf('! '));
        if (s.length && /[.!?]$/.test(s)) return s;
        return end > s.length * 0.6 ? s.slice(0, end + 1) : s + ' …';
      }
      function join(list, n){
        var out = [], left = n;
        for (var i = 0; i < list.length && left > 15; i++) { var c = clip(list[i], left); out.push(c); left -= count(c); }
        return out.join('\n');
      }
      var ROADMAP = /\b((the )?(rest|remainder) of (this|the) (paper|article|manuscript)|(is|are) (organi[sz]ed|structured) as follows|section \d+ (describes|presents|discusses|introduces))/i;
      var AIMS = /\b(here,? we|in this (study|paper|work|article|report)|in the present (study|work)|this (study|paper|work|article) (aims|aimed|presents|investigates|reports|describes|proposes)|we (aim|aimed|sought|set out|investigate|investigated|examine|examined|present|propose|show|report|develop|developed|hypothesi[sz]e|asked|ask|test|tested|study|studied|analy[sz]e|analy[sz]ed))\b/i;
      // The text the summary is made from, within maxWords: the abstract, the aims (the end of the
      // introduction), the first paragraphs of the discussion and of the conclusion; a paper without
      // those sections gives its opening paragraphs. `abstract` stands in for a page without one.
      window.luntaSummarySource = function(maxWords, abstract){
        var B = Math.max(100, maxWords || 1900), paras = paragraphs(), parts = [];
        function of(kind){ return paras.filter(function(p){ return p.kind === kind; }); }
        // the first paragraphs of the first such section (and its subsections): a lead-in ("This
        // section discusses …") is passed over, and short paragraphs bring the next along
        var LEADIN = /^((in )?this section|in the following|here,? we (discuss|summari[sz]e)|we (now |next )?(discuss|summari[sz]e))\b/i;
        function firstOf(kind){
          var list = of(kind), out = [], n = 0;
          for (var i = 0; i < list.length && out.length < 3 && n < 120; i++) {
            if (count(list[i].text) < 50 && LEADIN.test(list[i].text)) continue;
            out.push(list[i].text); n += count(list[i].text);
          }
          return out;
        }
        var abs = of('abstract').map(function(p){ return p.text; });
        if (!abs.length && squash(abstract)) abs = [squash(abstract)];
        // the aims: the introduction's last paragraph that says what was done ("Here we …"), else its
        // last paragraph that isn't the paper's roadmap; a list it introduces comes along
        var intro = of('intro'), aims = [];
        if (intro.length) {
          var tail = intro.filter(function(p){ return !p.item; }).slice(-4).filter(function(p){ return !ROADMAP.test(p.text); });
          var hit = tail.filter(function(p){ return AIMS.test(p.text); });
          var pick = hit.length ? hit[hit.length - 1] : tail[tail.length - 1];
          if (pick) {
            aims = [pick.text];
            for (var j = intro.indexOf(pick) + 1; j < intro.length && intro[j].item && /:$/.test(pick.text); j++) aims.push(intro[j].text);
          }
        }
        var disc = firstOf('discussion'), concl = firstOf('conclusion');
        var res = disc.length || concl.length ? [] : firstOf('results');
        if (abs.length) parts.push({ kind: 'abstract', label: 'Abstract', text: join(abs, Math.round(B * 0.4)) });
        if (aims.length) parts.push({ kind: 'aims', label: 'From the end of the introduction (the aims)', text: join(aims, Math.round(B * 0.15)) });
        if (res.length) parts.push({ kind: 'results', label: 'From the results', text: join(res, Math.round(B * 0.3)) });
        if (disc.length) parts.push({ kind: 'discussion', label: 'From the discussion', text: join(disc, Math.round(B * (concl.length ? 0.3 : 0.4))) });
        if (concl.length) parts.push({ kind: 'conclusion', label: 'From the conclusion', text: join(concl, Math.round(B * (disc.length ? 0.15 : 0.35))) });
        var used = parts.reduce(function(a, p){ return a + count(p.text); }, 0);
        // no discussion, conclusion or results to go by: the paper from its start
        if (!disc.length && !concl.length && !res.length) {
          var body = paras.filter(function(p){ return p.kind !== 'abstract' && p.kind !== 'back'; }).map(function(p){ return p.text; });
          if (body.length && B - used > 40) { parts.push({ kind: 'body', label: abs.length ? 'From the paper' : 'The beginning of the paper', text: join(body, B - used) }); }
        }
        var text = parts.map(function(p){ return p.label + ':\n' + p.text; }).join('\n\n');
        return JSON.stringify({ text: text, parts: parts.map(function(p){ return p.kind; }), words: count(squash(text)) });
      };

      // ---- the card ----
      function el(tag, cls, t){ var e = document.createElement(tag); e.setAttribute('class', cls); if (t != null) e.setAttribute('data-t', t); return e; }
      function post(m){ try { window.webkit.messageHandlers.summary.postMessage(m); } catch (x) {} }
      function card(){ return document.querySelector('.m-summary'); }
      // under the title, authors and journal line; or under the first heading
      function place(c){
        var tp = document.querySelector('.titlepage'), at = null, parent = null;
        if (tp) {
          parent = tp;
          tp.querySelectorAll('h1, .title, .authors, .journal, .doi').forEach(function(e){ if (e.parentNode === tp) at = e; });
          if (!at) at = tp.firstElementChild;
        } else {
          var h = document.querySelector('h1');
          if (h) { parent = h.parentNode; at = h; } else { parent = document.body; at = null; }
        }
        if (at) parent.insertBefore(c, at.nextSibling); else parent.insertBefore(c, parent.firstChild || null);
      }
      function build(){
        var c = el('div', 'm-summary');
        c.setAttribute('data-lunta-ui', 'summary');
        c.setAttribute('role', 'note');
        var head = el('div', 'm-sum-head');
        head.appendChild(el('div', 'm-sum-title', 'Summary'));
        var more = el('button', 'm-sum-more'); more.setAttribute('type', 'button'); more.setAttribute('aria-label', 'Summary options'); more.setAttribute('title', 'Summary options');
        head.appendChild(more);
        c.appendChild(head);
        c.appendChild(el('div', 'm-sum-text', ''));
        c.appendChild(el('div', 'm-sum-note', ''));
        var pop = el('div', 'm-sum-pop');
        [['regenerate', 'Regenerate'], ['hide', 'Hide Summaries']].forEach(function(a){
          var b = el('button', 'm-sum-item', a[1]); b.setAttribute('type', 'button'); b.setAttribute('data-act', a[0]); b.setAttribute('aria-label', a[1]);
          pop.appendChild(b);
        });
        c.appendChild(pop);
        return c;
      }
      function rect(e){ return e && e.getBoundingClientRect ? e.getBoundingClientRect() : null; }
      // a change to the card keeps the place being read: above the screen, the page scrolls by as
      // much as the card grew; in pages, the place is laid out again on its page
      function keep(change){
        var paged = window.luntaPaged && window.luntaPaged();
        if (paged) {
          var info = window.luntaPageInfo && window.luntaPageInfo();
          var at = info && info.page > 0 && window.luntaPlace ? window.luntaPlace() : null;
          change();
          if (at != null && window.luntaKeepPlace && window.luntaTargetPage) { window.luntaKeepPlace(at); window.luntaTargetPage(); }
          return;
        }
        var c = card(), r = rect(c), h0 = r ? r.height : 0;
        var above = r ? r.bottom <= 0 : false;
        change();
        c = card(); var r2 = rect(c), h1 = r2 ? r2.height : 0;
        if (!r && r2) above = r2.top < 0;
        if (above && h1 !== h0 && window.scrollBy) window.scrollBy(0, h1 - h0);
      }
      // {state: 'loading' | 'writing' | 'done', text, note}
      window.luntaSummaryShow = function(json){
        var s = typeof json === 'string' ? JSON.parse(json) : json;
        keep(function(){
          var c = card();
          if (!c) { c = build(); place(c); }
          c.classList.toggle('m-sum-loading', s.state === 'loading');
          c.querySelector('.m-sum-text').setAttribute('data-t', s.state === 'loading' ? (s.text || 'Summarizing…') : (s.text || ''));
          c.querySelector('.m-sum-note').setAttribute('data-t', s.state === 'done' ? (s.note || '') : '');
          c.setAttribute('aria-label', 'Summary');
        });
        return true;
      };
      window.luntaSummaryHide = function(){
        var c = card(); if (!c) return false;
        keep(function(){ c.remove(); });
        return true;
      };
      // the ⋯ menu: Regenerate, Hide Summaries (the app does them). Taps on the card do nothing
      // else (no page turn, no paragraph to listen from).
      if (document.addEventListener) document.addEventListener('click', function(e){
        var t = e.target, c = t && t.closest ? t.closest('.m-summary') : null;
        var open = document.querySelector('.m-summary.m-sum-open');
        if (!c) { if (open) open.classList.remove('m-sum-open'); return; }
        e.preventDefault(); e.stopImmediatePropagation();
        var item = t.closest('.m-sum-item');
        if (item) { c.classList.remove('m-sum-open'); post(item.getAttribute('data-act')); return; }
        if (t.closest('.m-sum-more')) c.classList.toggle('m-sum-open');
        else c.classList.remove('m-sum-open');
      }, true);
    })();
    """#
}
