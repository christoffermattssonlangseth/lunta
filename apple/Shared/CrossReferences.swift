import Foundation

// Cross-references in the text: "chapter 8", "Chapters 2–9", "section 10.4.1", "(see section 4.1)",
// "§2.3", "appendix C", "Appendix B.2", "problem 3.5", "Table 2", "equation 2.3", "Eq. (2.3)" and, in
// papers, "Section 2" and "Supplementary Note 3" become tappable, like figure mentions
// (FigureViewer.swift), when the book or paper has that target: a numbered heading ("8 Measuring
// performance", "10.4.1 Downsampling", "C Probability", "Chapter 3 …", "Appendix B …"), a problem's or
// exercise's label at the start of its paragraph, a table's caption or an equation's number tag. A
// tap goes there, with Back (JumpBack.swift) to return. Numbers alone ("(2.3)", "8 chapters") are
// never linked.
//
// The mentions are wrapped in spans and nothing else changes: the page's text, and so the offsets
// highlights and reading places are stored by, stay exactly as they were.
//
// A book read chapter by chapter (ChapterReader.swift): each part's page carries the whole book's
// targets as keys with the part each is in (`<meta name="lunta-xrefs">`, written by the core's
// `epub::xref_keys`, made the same way as here). A mention whose target is in another part is a link
// to that part's page (`lunta-part-<n>.xhtml#lunta-xref-<key>`), which the reader opens as it opens
// the contents' links, and goes to the target there (`luntaXrefGo`).

extension ReaderAssets {
    static let xrefJS = #"""
    (function(){
      if (!document.body || window.luntaXrefs) return;
      var st = document.createElement('style');
      st.textContent = '.m-xref { color: #2a6478; cursor: pointer; text-decoration: underline; text-decoration-color: rgba(42,100,120,.35);' +
        ' text-decoration-thickness: 1px; text-underline-offset: 2px; -webkit-tap-highlight-color: rgba(42,100,120,.18); border-radius: 2px; }' +
        '@media (prefers-color-scheme: dark) { .m-xref { color: #9ec9d8; text-decoration-color: rgba(158,201,216,.35); -webkit-tap-highlight-color: rgba(158,201,216,.2); } }' +
        'html[data-theme="paper"] .m-xref { color: #8a4a1c; text-decoration-color: rgba(138,74,28,.35); }' +
        '.m-xref-flash { background: rgba(255, 204, 0, .3) !important; border-radius: 4px; transition: background .6s; }';
      document.documentElement.appendChild(st);

      function norm(s){ return (s || '').replace(/\s+/g, ' ').trim(); }
      // a target's number: "8", "10.4.1", "S2", an appendix's "C" or "B.2"
      var NUM = '(?:S?\\d+(?:\\.\\d+)*|[A-Z](?:\\.\\d+)*)';
      // the same rules as the core's epub::xref_keys (it finds the targets in the other parts of a book)
      var HEAD = new RegExp('^(?:([Cc]hapter|CHAPTER|[Ss]ection|SECTION|[Aa]ppendix|APPENDIX|[Ss]upplementary\\s+[Nn]ote|SUPPLEMENTARY\\s+NOTE)\\s+)?(' + NUM + ')[.:]?(?:\\s+(\\S)|$)');
      var PROBLEM = /^\s*(?:Problem|Exercise)\s+(\d+(?:\.\d+)*|[A-Z](?:\.\d+)+)\s*$/;
      var TABLE = new RegExp('^(Supplementary\\s+|Suppl?\\.\\s*|Extended\\s+Data\\s+)?Table\\s+(S?\\d+(?:\\.\\d+)*|[A-Z](?:\\.\\d+)+)(?![A-Za-z0-9])');
      var EQTAG = /^\s*\(((?:\d+(?:\.\d+)*|[A-Z](?:\.\d+)+)[a-z]?)\)\s*$/;
      function lettered(n){ return /^[A-Z](\.|$)/.test(n); }
      function tableKey(pre, n){
        pre = (pre || '').toLowerCase();
        if (pre.indexOf('ext') === 0) return 'E' + n;
        return pre && n.charAt(0) !== 'S' ? 'S' + n : n;
      }
      function headKeys(text){
        var m = HEAD.exec(norm(text)); if (!m) return [];
        var n = m[2], pre = (m[1] || '').split(/\s+/)[0].toLowerCase(), letter = n.length === 1 && lettered(n);
        if (!pre) {
          var c = m[3] ? m[3].charAt(0) : '';
          return letter && !(/[0-9]/.test(c) || (c && c.toUpperCase() === c && c.toLowerCase() !== c)) ? [] : [n];
        }
        if (pre === 'chapter') return n.indexOf('.') < 0 ? ['ch:' + n, n] : [n];
        if (pre === 'section') return [n];
        if (pre === 'appendix') return lettered(n) ? ['app:' + n, n] : ['app:' + n];
        return ['note:' + n.replace(/^S(?=\d)/, '')];
      }

      // the targets in this page: key -> element (the first of a key)
      var here = {};
      function add(k, el){ if (!(k in here)) here[k] = el; }
      document.querySelectorAll('h1, h2, h3, h4, h5, h6').forEach(function(h){ headKeys(h.textContent).forEach(function(k){ add(k, h); }); });
      document.querySelectorAll('strong, b').forEach(function(s){
        var m = PROBLEM.exec(s.textContent); if (!m) return;
        var p = s.parentNode, f = p && p.firstChild;
        while (f && f.nodeType === 3 && !f.nodeValue.trim()) f = f.nextSibling;
        if (f === s && p.closest && p.closest('p, div, li') === p) add('problem:' + m[1], p);
      });
      function capOf(box){
        for (var c = box.firstElementChild; c; c = c.nextElementSibling) if (/^(figcaption|caption)$/i.test(c.nodeName) || c.classList.contains('caption')) return c;
        return box.querySelector('figcaption, caption, .caption');
      }
      document.querySelectorAll('figure, .table-wrap, table').forEach(function(t){
        var o = t.parentNode && t.parentNode.closest ? t.parentNode.closest('figure, .table-wrap') : null;
        if (o) return;   // a table in its figure or wrapper: the outer box is the target
        var cap = capOf(t), m = cap ? TABLE.exec(norm(cap.textContent)) : null;
        if (m) add('table:' + tableKey(m[1], m[2]), t);
      });
      document.querySelectorAll('.ltx_tag_equation, .eqno').forEach(function(x){
        var m = EQTAG.exec(x.textContent); if (!m) return;
        add('eq:' + m[1], x.closest('.ltx_equation, .ltx_equationgroup, .disp-formula, .equation, tr, p, div') || x);
      });

      // a book read chapter by chapter: the other parts' targets, and which part this is
      var there = {}, mp = document.querySelector('meta[name="lunta-part"]'), part = mp ? mp.getAttribute('content') : null;
      var mx = document.querySelector('meta[name="lunta-xrefs"]');
      if (mx) { try { there = JSON.parse(mx.getAttribute('content') || '{}') || {}; } catch (x) { there = {}; } }
      function has(k){ return k in here || (k in there && String(there[k]) !== part); }
      // a lone letter is an appendix only in a book that has its sections ("C.1") or the letters
      // around it ("A Notation", "B Mathematics"), not a heading that starts "A Brief History"
      function letterOk(k){
        if (has('app:' + k)) return true;
        var c = k.charCodeAt(0), all = Object.keys(here).concat(Object.keys(there));
        for (var i = 0; i < all.length; i++) {
          var a = all[i];
          if (a.indexOf(k + '.') === 0) return true;
          if (a.length === 1 && Math.abs(a.charCodeAt(0) - c) === 1) return true;
        }
        return false;
      }
      // the keys a mention could mean, in order
      function candidates(kind, n, pre){
        switch (kind) {
          case 'ch': return /^\d+$/.test(n) ? ['ch:' + n, n] : [];
          case 'sec': return n.length === 1 && lettered(n) ? (letterOk(n) ? [n] : []) : [n];
          case 'app': return lettered(n) ? ['app:' + n].concat(n.length > 1 || letterOk(n) ? [n] : []) : ['app:' + n];
          case 'note': return /^S?\d+$/.test(n) ? ['note:' + n.replace(/^S/, '')] : [];
          case 'prob': return lettered(n) && n.length === 1 ? [] : ['problem:' + n];
          case 'tab': return ['table:' + tableKey(pre, n)];
          case 'eq': return /^(\d|[A-Z]\.\d)/.test(n) ? ['eq:' + n] : [];
        }
        return [];
      }
      function target(kind, n, pre){
        var c = candidates(kind, n, pre);
        for (var i = 0; i < c.length; i++) if (has(c[i])) return c[i];
        return null;
      }

      // the mentions: a word naming the kind of target, then its number (one or more for a plural)
      var KINDS = ['ch', 'sec', 'app', 'note', 'prob', 'tab', 'eq'];
      var WORDS = [
        '[Cc]hapters?|CHAPTERS?|Chs?\\.',
        '[Ss]ections?|SECTIONS?|[Ss]ecs?\\.|[Ss]ects?\\.|§§?',
        '[Aa]ppendix|[Aa]ppendices|APPENDIX|APPENDICES',
        '[Ss]upplementary\\s+[Nn]otes?|Suppl?\\.\\s*[Nn]otes?',
        '[Pp]roblems?|[Ee]xercises?',
        '(?:[Ss]upplementary\\s+|Suppl?\\.\\s*|Extended\\s+Data\\s+)?[Tt]ables?',
        '[Ee]quations?|[Ee]qns?\\.|[Ee]qs?\\.'
      ];
      var REF = '(\\(?)(' + NUM + ')(\\)?)(?![A-Za-z0-9]|\\.\\d)';
      var MENTION = new RegExp('(?<![A-Za-z\\-])(?:' + WORDS.map(function(w){ return '(' + w + ')'; }).join('|') + ')(\\s*)' + REF, 'g');
      var MORE = new RegExp('^(\\s*(?:,\\s*(?:and|or)\\s|,|;|and\\s|or\\s|&|to\\s|–|—|-)\\s*)' + REF);
      var PLURAL = /(s|ices|§§|Chs\.|Secs\.|Sects\.|Eqs\.|Eqns\.)$/;
      var QUICK = /[Cc]hapter|CHAPTER|Chs?\.|[Ss]ection|SECTION|[Ss]ects?\.|[Ss]ecs?\.|§|[Aa]ppendi|APPENDI|[Nn]ote|[Pp]roblem|[Ee]xercise|[Tt]able|[Ee]q/;
      var SKIP = 'a, script, style, code, pre, kbd, samp, math, nav, title, button, h1, h2, h3, h4, h5, h6, .m-figref, .m-xref, .m-figcard, .references, ol.refs, .refs, .index';

      function scan(s){
        var out = [], m;
        MENTION.lastIndex = 0;
        while ((m = MENTION.exec(s))) {
          var k = 0; while (k < KINDS.length && m[k + 1] === undefined) k++;
          var kind = KINDS[k], word = m[k + 1], gap = m[8], open = m[9], n = m[10], close = m[11];
          var end = m.index + m[0].length;
          if (close && !open) end -= 1;   // "(see section 4.1)": the bracket is the sentence's
          // "Chapter8" isn't a mention; "§2.3" and "Eq.(2.3)" are. Brackets only round an equation's number.
          if ((!gap && /[A-Za-z]$/.test(word)) || (open && (kind !== 'eq' || !close))) {
            MENTION.lastIndex = Math.max(m.index + 1, end);
            continue;
          }
          var pre = kind === 'tab' ? (/^(supplementary|suppl?\.|extended)/i.exec(word) || [''])[0] : '';
          var refs = [{ at: m.index, end: end, key: target(kind, n, pre) }];
          var pos = end;
          if (PLURAL.test(word.replace(/\s+/g, ' '))) {
            var mm;
            while ((mm = MORE.exec(s.slice(pos)))) {
              var sep = mm[1], o2 = mm[2], n2 = mm[3], c2 = mm[4];
              if (o2 && (kind !== 'eq' || !c2)) break;
              var at = pos + sep.length, e2 = pos + mm[0].length - (c2 && !o2 ? 1 : 0);
              // a range: "chapters 3–5" is chapter 3's link; "sections 3.2–3.4" both ends'
              var range = /[–—-]|to\s/.test(sep);
              refs.push({ at: at, end: e2, key: range && n2.indexOf('.') < 0 ? null : target(kind, n2, pre) });
              pos = e2;
            }
          }
          MENTION.lastIndex = pos;
          refs.forEach(function(r){ if (r.key) out.push(r); });
        }
        return out;
      }

      var count = 0;
      function link(root){
        var w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, null), t, list = [];
        while ((t = w.nextNode())) if (QUICK.test(t.nodeValue)) list.push(t);
        list.forEach(function(n){
          var p = n.parentNode; if (!p || !p.closest || p.closest(SKIP)) return;
          var s = n.nodeValue, out = scan(s).filter(function(r){
            var el = here[r.key];
            return !(el && el.contains(n));   // not a problem's own label or a table's own caption
          });
          if (!out.length) return;
          var frag = document.createDocumentFragment(), cur = 0;
          out.forEach(function(o){
            if (o.at > cur) frag.appendChild(document.createTextNode(s.slice(cur, o.at)));
            var sp = document.createElement('span');
            sp.className = 'm-xref'; sp.setAttribute('data-xref', o.key);
            if (!(o.key in here)) sp.setAttribute('data-xref-part', there[o.key]);
            sp.textContent = s.slice(o.at, o.end);
            frag.appendChild(sp); cur = o.end; count++;
          });
          if (cur < s.length) frag.appendChild(document.createTextNode(s.slice(cur)));
          p.replaceChild(frag, n);
        });
      }
      link(document.body);

      function show(el, mark){
        if (mark && window.luntaMarkJump) window.luntaMarkJump();
        var head = /^h[1-6]$/i.test(el.nodeName);
        el.scrollIntoView({ behavior: 'smooth', block: head ? 'start' : 'center' });
        if (!head) { el.classList.add('m-xref-flash'); setTimeout(function(){ el.classList.remove('m-xref-flash'); }, 1800); }
        return true;
      }
      function go(key, inPart){
        if (inPart != null && inPart !== '') {
          // another part of the book: the reader opens it and calls luntaXrefGo there (ChapterReader.open(link:))
          window.location.href = 'lunta-part-' + inPart + '.xhtml#lunta-xref-' + key;
          return true;
        }
        return here[key] ? show(here[key], true) : false;
      }
      window.luntaXrefs = { count: function(){ return count; }, keys: function(){ return Object.keys(here); }, go: go };
      // the target `key` in this page, arriving from another part
      window.luntaXrefGo = function(key){ return here[key] ? show(here[key], false) : false; };

      document.addEventListener('click', function(e){
        var t = e.target, x = t && t.closest ? t.closest('.m-xref') : null;
        if (!x) return;
        if (window.luntaExtending && window.luntaExtending()) return;   // a tap ending a highlight
        var sel = window.getSelection ? window.getSelection() : null;
        if (sel && !sel.isCollapsed) return;
        if (!go(x.getAttribute('data-xref'), x.getAttribute('data-xref-part'))) return;
        e.preventDefault(); e.stopImmediatePropagation();
      }, true);
    })();
    """#
}
