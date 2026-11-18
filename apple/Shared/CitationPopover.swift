import Foundation

/// Tapping a citation ("[12]", "Smith et al., 2020") shows the reference in a small card next to
/// it instead of jumping to the reference list: the reference, "Open paper" (its DOI), "Add to
/// Lunta" (or open it, if it's already in the library) and "Show in References". Tapping outside,
/// scrolling or Esc closes it, and the reader stays where it was.
///
/// The card is attached to <html>, outside <body>, so it never becomes part of the page text that
/// highlights and reading positions are counted in. "Add to Lunta" is a lunta://cite?doi=… link
/// (?arxiv=…, or ?ref=<the reference's text> without either: the app looks its DOI up), handled by
/// the app (`ExternalLinks.onLunta`), which reports back with `luntaCiteDone(text)`.
extension ReaderAssets {
    static let citationJS = #"""
    (function(){
      if (window.__luntaCite) return; window.__luntaCite = true;
      var st = document.createElement('style');
      st.textContent = '.m-cite { position: fixed; z-index: 2147483000; max-width: min(30em, 92vw); max-height: 45vh; overflow: auto; ' +
        'padding: 12px 14px 10px; border-radius: 12px; border: 1px solid rgba(127,127,127,.35); box-shadow: 0 10px 30px rgba(0,0,0,.22); ' +
        'font: 15px/1.45 -apple-system, "Helvetica Neue", sans-serif; -webkit-text-size-adjust: 100%; }' +
        '.m-cite-ref { margin: 0 0 10px; overflow-wrap: anywhere; }' +
        '.m-cite-ref a { color: inherit; text-decoration: none; pointer-events: none; }' +
        '.m-cite-actions { display: flex; flex-wrap: wrap; gap: 8px; }' +
        '.m-cite-btn { font: 600 13px -apple-system, "Helvetica Neue", sans-serif; padding: 5px 10px; border-radius: 8px; text-decoration: none; ' +
        'color: #2a6478; background: rgba(42,100,120,.10); -webkit-tap-highlight-color: transparent; }' +
        '.m-cite-sep { border-top: 1px solid rgba(127,127,127,.25); margin: 10px 0; } ' +
        '@media (prefers-color-scheme: dark) { .m-cite { box-shadow: 0 10px 30px rgba(0,0,0,.6); } .m-cite-btn { color: #9ec9d8; background: rgba(158,201,216,.14); } }';
      document.documentElement.appendChild(st);

      var pop = null, pending = null;   // the card; the Add button pressed last
      // what the card knows about the library: DOIs and arXiv numbers in it (from the app), and how
      // "Add to Lunta" went for references added since the page opened (by the reference's key)
      var inLib = {}, known = {};
      window.luntaSetLibrary = function(ids){ inLib = {}; (ids || []).forEach(function(k){ inLib[String(k).toLowerCase()] = true; }); };

      // the reference a link points to, if it points into the reference list
      function refTarget(a){
        var h = a.getAttribute('href') || '', i = h.indexOf('#');
        if (i < 0) return null;
        var id = h.slice(i + 1); try { id = decodeURIComponent(id); } catch (x) {}
        if (!id) return null;
        var t = document.getElementById(id); if (!t) return null;
        if (t.closest('figure, table, .table-wrap, h1, h2, h3, h4')) return null;
        var inRefs = !!t.closest('ol.refs, ol.references, ul.references, .ref-list, .references, .bibliography, #references, .ltx_bibliography, .ltx_biblist');
        if (!inRefs) {
          var sec = t.closest('section');
          var hd = sec && sec.querySelector('h1, h2, h3');
          inRefs = !!(hd && /^\s*(references|bibliography|literature cited|works cited|references and notes)\b/i.test(hd.textContent));
        }
        return inRefs ? (t.closest('li') || t) : null;
      }

      // A reference without the publisher's link buttons: bioRxiv/HighWire pages put "↵" (back to
      // the text), "OpenUrl", "CrossRef", "PubMed", "Abstract/FREE Full Text", "Google Scholar"
      // links in each entry, kept as text by "Use this page's full text"; and the "[3]." label.
      var LINK_LABEL = /^\s*(↵|↩|\^|↑|OpenUrl|CrossRef|Crossref|PubMed|Medline|MEDLINE|PMC|PubMed Central|PMC free article|Google Scholar|Web of Science|Scopus|Abstract|Full Text|FREE Full Text|Abstract\/FREE Full Text|Article|CAS|ADS|ISI|View Article|Find this author on (Google Scholar|PubMed))\s*$/i;
      function cleaned(el){
        var c = el.cloneNode(true);
        c.querySelectorAll('a').forEach(function(a){
          var href = a.getAttribute('href') || '';
          if (LINK_LABEL.test(a.textContent || '') || /^#|#xref-|\.xhtml#xref/.test(href) && /^\s*[↵↩^↑]?\s*$/.test(a.textContent || '')) a.remove();
        });
        // the entry's own label at its start ("[3].", "3.") when the list numbers it already
        (function first(node){
          for (var i = 0; i < node.childNodes.length; i++) {
            var n = node.childNodes[i];
            if (n.nodeType === 3) {
              if (!(n.data || '').trim()) continue;
              n.data = n.data.replace(/^\s*\[\d+\]\.?\s*|^\s*\d+\.\s+/, '');
              return true;
            }
            if (n.nodeType === 1 && first(n)) return true;
          }
          return false;
        })(c);
        return c;
      }

      function doiOf(el){
        var a = el.querySelector('a[href*="doi.org/"]');
        if (a) { var m = a.getAttribute('href').match(/doi\.org\/(10\.[^\s?#]+)/i); if (m) { try { return decodeURIComponent(m[1]); } catch (x) { return m[1]; } } }
        // HighWire's CrossRef button: /lookup/external-ref?access_num=10.…&link_type=DOI
        var cr = el.querySelector('a[href*="link_type=DOI"]');
        if (cr) { var m3 = cr.getAttribute('href').match(/access_num=(10\.[^&]+)/); if (m3) { try { return decodeURIComponent(m3[1]); } catch (x) { return m3[1]; } } }
        var m2 = (el.textContent || '').match(/\b(10\.\d{4,9}\/[^\s"<>]+)/);
        return m2 ? m2[1].replace(/[.,;)\]]+$/, '') : '';
      }

      // numbers in a citation: "1,2", "11–13", "3, 5-7" (ranges spelled out); null if it isn't one
      function citeNumbers(t){
        if (!/^\s*\d{1,3}(\s*[,;–—\-]\s*\d{1,3})*\s*$/.test(t)) return null;
        var out = [];
        t.replace(/(\d{1,3})(?:\s*[–—\-]\s*(\d{1,3}))?/g, function(_, x, y){
          var a = +x, b = y ? +y : a;
          if (b < a || b - a > 60) b = a;
          for (var n = a; n <= b; n++) out.push(n);
        });
        return out.length ? out : null;
      }
      // the number under the click in a citation's text (by character offset), else the first
      function numberAt(t, off){
        var re = /\d{1,3}/g, m, first = null;
        while ((m = re.exec(t))) { if (first === null) first = +m[0]; if (off != null && off >= m.index && off <= m.index + m[0].length) return +m[0]; }
        return first;
      }
      // reference n of the paper's numbered list: the entry printed "n." / "[n]" / "n ", else the n-th
      function refNumbered(n){
        var list = document.querySelector('ol.refs, ol.references, ul.references');
        if (!list) return null;
        var items = Array.prototype.filter.call(list.children || [], function(x){ return x.tagName && x.tagName.toLowerCase() === 'li'; });
        var labelled = false;
        for (var i = 0; i < items.length; i++) {
          var m = (items[i].textContent || '').match(/^\s*\[?(\d{1,3})[\].)]?\s/);
          if (m) { labelled = true; if (+m[1] === n) return items[i]; }
        }
        return !labelled && items[n - 1] ? items[n - 1] : null;
      }
      // A linked citation's whole group: "[1-3]" with links on 1 and 3, a superscript "11–13": the
      // references of all its numbers, found from the clicked one's place in the list
      function group(a, ref){
        var n0 = +((a.textContent || '').trim());
        if (!(n0 > 0)) return [ref];
        var ctx = null, sup = a.closest('sup');
        if (sup) ctx = sup.textContent || '';
        else {
          // the text around the link, within its brackets
          var par = a.parentNode, before = '', after = '', seen = false;
          (par ? par.childNodes : []).forEach(function(k){
            if (k === a || (k.contains && k.contains(a))) { seen = true; return; }
            var t = k.textContent || ''; if (seen) after += t; else before += t;
          });
          var l = before.lastIndexOf('['), r = after.indexOf(']');
          if (l < 0 || r < 0 || before.length - l > 40 || r > 40) return [ref];
          ctx = before.slice(l + 1) + a.textContent + after.slice(0, r);
        }
        var ns = citeNumbers(ctx.replace(/[()\[\]]/g, ''));
        if (!ns || ns.length < 2 || ns.indexOf(n0) < 0) return [ref];
        var li = ref.closest ? (ref.closest('li') || ref) : ref, list = li.parentNode;
        if (!list) return [ref];
        var items = Array.prototype.filter.call(list.children || [], function(x){ return x.tagName && x.tagName.toLowerCase() === 'li'; });
        var at = items.indexOf(li);
        if (at < 0) return [ref];
        var out = ns.map(function(n){ return items[at + (n - n0)]; }).filter(Boolean);
        return out.length ? out : [ref];
      }
      // ---- author–year citations: "(Jaume et al., 2024)", "(Smith and Jones, 2019; Lee, 2020a)",
      // "Jaume et al. (2024)" — matched to the reference list by the first author's surname and year
      function fold(t){ return (t || '').normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase(); }
      // the reference list's entries: its list items, else the paragraphs under a References heading
      function refEntries(){
        var hs = Array.prototype.filter.call(document.querySelectorAll('h1, h2, h3'), function(h){
          return /^\s*(references|bibliography|literature cited|works cited|references and notes)\b/i.test(h.textContent || ''); });
        var out = [];
        hs.forEach(function(h){
          var sec = h.closest('section');
          var scope = sec || h.parentNode;
          if (!scope) return;
          Array.prototype.forEach.call(scope.querySelectorAll('li, p'), function(e){
            if (e.closest('figure, table') || (e.tagName.toLowerCase() === 'p' && e.closest('li'))) return;
            if ((e.textContent || '').length > 20) out.push(e);
          });
        });
        if (!out.length) Array.prototype.forEach.call(document.querySelectorAll('ol.refs > li, ol.references > li, ul.references > li'), function(e){ out.push(e); });
        return out;
      }
      function refByAuthor(entries, surname, year){
        var who = fold(surname), y = year.replace(/[a-z]$/, ''), best = null;
        for (var i = 0; i < entries.length; i++) {
          var t = entries[i].textContent || '', ft = fold(t).replace(/^\s*(\[\d+\]|\d+\.)\s*/, '');
          if (t.indexOf(y) < 0) continue;
          // the first author: up to the first comma, or "and", in the entry's first words
          var first = ft.slice(0, 80).split(/,| and | & |\(/)[0];
          var re = new RegExp('(^|[^a-z])' + who.replace(/[^a-z]/g, '.') + '([^a-z]|$)');
          if (re.test(first)) { if (t.indexOf(year) >= 0) return entries[i]; best = best || entries[i]; }
        }
        return best;
      }
      function authorYear(c){
        var d = c.node.data || '', o = c.offset;
        var l = d.lastIndexOf('(', o), r = d.indexOf(')', Math.max(o - 1, 0));
        if (l < 0 || r < 0 || r < l || r - l > 200 || o < l || o > r + 1) return null;
        var inner = d.slice(l + 1, r), YEAR = /((?:1[89]|20)\d\d[a-z]?)/;
        var parts = [];
        if (/^\s*(?:1[89]|20)\d\d[a-z]?\s*$/.test(inner)) {
          // narrative: "Jaume et al. (2024)", "Smith and Jones (2019)"
          var before = d.slice(Math.max(0, l - 60), l), m = before.match(/([A-Z][A-Za-z\u00C0-\u024F'\-]+)(?:\s+et\s+al\.?|\s+(?:and|&)\s+[A-Z][A-Za-z\u00C0-\u024F'\-]+)?\s*$/);
          if (m) parts.push([m[1], inner.trim()]);
        } else {
          inner.split(/;/).forEach(function(p){
            var m = p.match(/([A-Z][A-Za-z\u00C0-\u024F'\-]+)(?:\s+et\s+al\.?|\s+(?:and|&)\s+[A-Z][A-Za-z\u00C0-\u024F'\-]+)?,?\s+(?:e\.g\.,?\s*)?((?:1[89]|20)\d\d[a-z]?)/);
            if (m) parts.push([m[1], m[2]]);
          });
        }
        if (!parts.length) return null;
        var p0 = c.node.parentNode;
        if (p0 && p0.closest && p0.closest('ol.refs, ol.references, .references, figure, table')) return null;
        var entries = refEntries();
        if (!entries.length) return null;
        var refs = [];
        parts.forEach(function(pt){ var e = refByAuthor(entries, pt[0], pt[1]); if (e && refs.indexOf(e) < 0) refs.push(e); });
        if (!refs.length) return null;
        var range = document.createRange(); range.setStart(c.node, l); range.setEnd(c.node, r + 1);
        return { anchor: { getBoundingClientRect: function(){ return range.getBoundingClientRect(); } }, ref: refs };
      }
      function caretAt(ev){
        if (document.caretRangeFromPoint) { var r = document.caretRangeFromPoint(ev.clientX, ev.clientY); if (r) return { node: r.startContainer, offset: r.startOffset }; }
        if (document.caretPositionFromPoint) { var p = document.caretPositionFromPoint(ev.clientX, ev.clientY); if (p) return { node: p.offsetNode, offset: p.offset }; }
        return null;
      }
      function plainCitation(ev){
        var t = ev.target;
        var sup = t && t.closest && t.closest('sup');
        var c = caretAt(ev);
        if (sup && !sup.closest('ol.refs, ol.references, .references, figure, table')) {
          var txt = sup.textContent || '';
          if (!citeNumbers(txt.replace(/[()\[\]]/g, ''))) return null;
          var off = null;
          if (c && c.node && sup.contains(c.node)) {
            // the offset within the superscript's text
            off = 0; var found = false;
            (function walk(n){ for (var i = 0; i < n.childNodes.length && !found; i++) { var k = n.childNodes[i];
              if (k === c.node) { off += c.offset; found = true; } else if (k.nodeType === 3) off += (k.data || '').length; else walk(k); } })(sup);
            if (!found) off = null;
          }
          var all = citeNumbers(txt.replace(/[()\[\]]/g, '')).map(refNumbered).filter(Boolean);
          return all.length ? { anchor: sup, ref: all } : null;
        }
        // "[1-3]" in running text; else an author–year citation
        if (!c || !c.node || c.node.nodeType !== 3) return null;
        var ay = authorYear(c);
        if (ay) return ay;
        var d = c.node.data || '', o = c.offset;
        var l = d.lastIndexOf('[', o), r = d.indexOf(']', Math.max(o - 1, 0));
        if (l < 0 || r < 0 || r < l || r - l > 40 || o < l || o > r + 1) return null;
        var inner = d.slice(l + 1, r);
        if (!citeNumbers(inner)) return null;
        var p = c.node.parentNode;
        if (p && p.closest && p.closest('ol.refs, ol.references, .references, figure, table')) return null;
        var ref2 = citeNumbers(inner).map(refNumbered).filter(Boolean);
        if (!ref2.length) return null;
        var range = document.createRange(); range.setStart(c.node, l); range.setEnd(c.node, r + 1);
        return { anchor: { getBoundingClientRect: function(){ return range.getBoundingClientRect(); } }, ref: ref2 };
      }

      function close(){ if (pop) { pop.remove(); pop = null; } }
      window.luntaCiteDone = function(text){
        if (!pop) return;
        var b = (pending && pop.contains(pending)) ? pending : pop.querySelector('.m-cite-lunta'); if (b) { b.textContent = text; b.removeAttribute('href');
          var k = b.getAttribute('data-key'); if (k && !/couldn|nothing|fail|error/i.test(text)) known[k] = text; }
      };

      // the card: one reference, or each reference of a citation like "11–13" / "[1-3]" in turn
      function open(a, refs){
        close();
        if (!refs.length) refs = [refs];
        refs = refs.slice(0, 12);
        pop = document.createElement('div'); pop.className = 'm-cite';
        var bs = getComputedStyle(document.body);
        var bg = bs.backgroundColor; if (!bg || bg === 'rgba(0, 0, 0, 0)' || bg === 'transparent') bg = getComputedStyle(document.documentElement).backgroundColor;
        pop.style.background = (bg && bg !== 'rgba(0, 0, 0, 0)') ? bg : '#fff';
        pop.style.color = bs.color;
        function section(ref){
        var doi = doiOf(ref);
        var body = document.createElement('div'); body.className = 'm-cite-ref';
        var copy = cleaned(ref);
        copy.querySelectorAll('[id]').forEach(function(x){ x.removeAttribute('id'); });
        copy.removeAttribute && copy.removeAttribute('id');
        body.innerHTML = copy.innerHTML;
        pop.appendChild(body);
        var acts = document.createElement('div'); acts.className = 'm-cite-actions';
        function btn(label, href, cls){ var b = document.createElement('a'); b.className = 'm-cite-btn' + (cls ? ' ' + cls : ''); b.textContent = label; b.setAttribute('href', href); acts.appendChild(b); return b; }
        // Add to Lunta, or what is already known: added since the page opened, or in the library
        // (then it opens the paper: the app opens a paper it already has)
        function lunta(key, href, inLibrary){
          var b = btn(known[key] || (inLibrary ? 'In library · Open' : 'Add to Lunta'), href, 'm-cite-lunta');
          b.setAttribute('data-key', key);
          if (known[key]) b.removeAttribute('href');
          return b;
        }
        if (doi) {
          btn('Open paper', 'https://doi.org/' + doi);
          var l = lunta('doi:' + doi.toLowerCase(), 'lunta://cite?doi=' + encodeURIComponent(doi), inLib[doi.toLowerCase()]);
          l.addEventListener('click', function(){ pending = l; var opening = /In library/.test(l.textContent); setTimeout(function(){ if (pop && l.getAttribute('href')) l.textContent = opening ? 'Opening…' : 'Adding…'; }, 0); });
        }
        else {
          // no DOI: an arXiv number, else a search for the reference
          var ax = (ref.textContent || '').match(/arXiv:\s*(\d{4}\.\d{4,5})(v\d+)?/i);
          if (ax) {
            btn('Open paper', 'https://arxiv.org/abs/' + ax[1]);
            var la = lunta('arxiv:' + ax[1], 'lunta://cite?arxiv=' + ax[1], inLib['arxiv:' + ax[1]] || inLib['10.48550/arxiv.' + ax[1]]);
            la.addEventListener('click', function(){ pending = la; setTimeout(function(){ if (pop && la.getAttribute('href')) la.textContent = 'Adding…'; }, 0); });
          } else {
            // neither: the app looks the reference up (Crossref's bibliographic search) and adds what it finds
            var full = (copy.textContent || '').replace(/\s+/g, ' ').trim();
            var lr = lunta('ref:' + full.slice(0, 160), 'lunta://cite?ref=' + encodeURIComponent(full.slice(0, 600)), false);
            lr.addEventListener('click', function(){ pending = lr; setTimeout(function(){ if (pop && lr.getAttribute('href')) lr.textContent = 'Looking it up…'; }, 0); });
            btn('Search', 'https://scholar.google.com/scholar?q=' + encodeURIComponent(full.slice(0, 200)));
          }
        }
        var go = btn('Show in References', '#');
        go.addEventListener('click', function(ev){
          ev.preventDefault(); ev.stopPropagation(); close();
          if (ref.id) location.hash = ref.id; else ref.scrollIntoView({ block: 'center' });
        });
        pop.appendChild(acts);
        }
        refs.forEach(function(ref, k){ if (k) { var hr = document.createElement('div'); hr.className = 'm-cite-sep'; pop.appendChild(hr); } section(ref); });

        // outside <body>: never part of the page text
        document.documentElement.appendChild(pop);
        var r = a.getBoundingClientRect(), vw = window.innerWidth, vh = window.innerHeight;
        var w = pop.offsetWidth, h = pop.offsetHeight;
        var left = Math.max(8, Math.min(vw - w - 8, r.left + r.width / 2 - w / 2));
        var top = (r.bottom + 8 + h < vh - 8 || r.top < vh / 2) ? r.bottom + 8 : r.top - h - 8;
        pop.style.left = left + 'px'; pop.style.top = Math.max(8, Math.min(vh - h - 8, top)) + 'px';
      }

      document.addEventListener('click', function(ev){
        if (pop && pop.contains(ev.target)) return;            // the card's own buttons
        var wasOpen = !!pop;
        close();
        var a = ev.target.closest && ev.target.closest('a[href]');
        var ref = a && !document.body.classList.contains('listening') ? refTarget(a) : null;
        var sel = window.getSelection();
        if (ref && !(sel && !sel.isCollapsed)) { ev.preventDefault(); ev.stopPropagation(); open(a, group(a, ref)); return; }
        // a citation that isn't a link (reading views made from a PDF): a superscript "1,2" / "11–13"
        // or "[1-3]" in the text, when the paper has a numbered reference list
        if (!a && !(sel && !sel.isCollapsed) && !document.body.classList.contains('listening')) {
          var pc = plainCitation(ev);
          if (pc) { ev.preventDefault(); ev.stopPropagation(); open(pc.anchor, pc.ref); return; }
        }
        // a tap that only closed the card does nothing else (no page turn, no highlight menu)
        if (wasOpen) { ev.preventDefault(); ev.stopPropagation(); }
      }, true);
      window.addEventListener('scroll', close, { passive: true });
      window.addEventListener('resize', close);
      document.addEventListener('keydown', function(ev){ if (ev.key === 'Escape') close(); });
    })();
    """#
}
