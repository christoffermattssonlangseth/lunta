import Foundation

/// The paper's reference list, for the reader's References panel: `luntaReferences()` returns it
/// as JSON (number, text, DOI / arXiv / PubMed ids, where it is, how often the text cites it) and
/// `luntaShowReference(i)` jumps to the i-th reference (with Back to return).
///
/// The reading views mark their reference lists differently: `ol.refs` (JATS/PMC, arXiv,
/// ScienceDirect, Atypon, web pages), `.ref-list` / `.references` / `.bibliography` lists on
/// journal pages, and, from PDFs, plain paragraphs after a "References" heading. Runs in every
/// reader (Mac, iPhone, iPad, Android, desktop: extract-reader-assets.mjs).
extension ReaderAssets {
    static let referencesJS = #"""
    (function(){
      if (window.luntaReferences) return;
      var HEAD = /^\s*([0-9IVX]+(\.\d+)*\.?\s+)?(references( and notes| cited)?|bibliography|literature( cited)?|works cited|cited literature|reference list|sources)\s*[:.]?\s*$/i;
      var cache = null;

      function clean(s){ return (s || '').replace(/\s+/g, ' ').trim(); }
      function isHeading(el){ return /^H[1-6]$/i.test(el.tagName); }
      function level(el){ return parseInt(el.tagName.slice(1), 10); }

      // the reference list's entries, in order; a book read chapter by chapter: those of its
      // reference list's chapter (window.luntaShelf, CitationPopover.swift) when this one has none
      function items(){
        var own = itemsIn(document.body);
        if (own.length || !window.luntaShelf || !window.luntaShelf.doc.body) return own;
        return itemsIn(window.luntaShelf.doc.body);
      }
      window.luntaReferenceIndex = function(el){ return items().indexOf(el); };
      function itemsIn(body){
        var out = [], seen = new Set();
        function add(el){
          if (!el || seen.has(el) || el.closest('.m-cite')) return;
          if (clean(el.textContent).length < 3) return;
          seen.add(el); out.push(el);
        }
        function listItems(list){
          for (var c = list.firstElementChild; c; c = c.nextElementSibling) if (/^LI$/i.test(c.tagName)) add(c);
        }
        if (!body) return out;
        // 1. lists the converters mark
        body.querySelectorAll('ol.refs, ul.refs, ol.references, ul.references, .ltx_biblist').forEach(listItems);
        // 2. lists inside a marked reference section (journal pages)
        if (!out.length) {
          body.querySelectorAll('.ref-list, .references, .bibliography, #references, [role="doc-bibliography"]').forEach(function(sec){
            sec.querySelectorAll('ol, ul').forEach(function(l){ if (!l.parentElement.closest('li')) listItems(l); });
          });
        }
        // 3. whatever follows a "References" heading, up to the next heading (PDFs: one paragraph each)
        if (!out.length) {
          body.querySelectorAll('h1, h2, h3, h4').forEach(function(h){
            if (!HEAD.test(clean(h.textContent))) return;
            var lv = level(h);
            for (var n = h.nextElementSibling; n; n = n.nextElementSibling) {
              if (isHeading(n) && level(n) <= lv) break;
              if (n.classList.contains('section-start')) break;
              if (isHeading(n)) continue;
              var tag = n.tagName.toUpperCase();
              if (tag === 'OL' || tag === 'UL') listItems(n);
              else if (tag === 'P' || tag === 'LI') add(n);
              else if (tag === 'DIV' || tag === 'SECTION') {
                var ls = n.querySelectorAll('ol, ul');
                if (ls.length) ls.forEach(listItems);
                else { var ps = n.querySelectorAll('p'); if (ps.length) ps.forEach(add); else add(n); }
              }
            }
          });
        }
        return out;
      }

      function decode(s){ try { return decodeURIComponent(s); } catch (x) { return s; } }
      function trimEnd(s){ return s.replace(/[.,;:)\]}>'"]+$/, ''); }

      function doiOf(el, text){
        var as = el.querySelectorAll('a[href]');
        for (var i = 0; i < as.length; i++) {
          var m = as[i].getAttribute('href').match(/doi\.org\/(10\.[^\s?#]+)/i);
          if (m) return trimEnd(decode(m[1]));
        }
        var m2 = text.match(/\b(10\.\d{4,9}\/[^\s"<>]+)/);
        return m2 ? trimEnd(m2[1]) : '';
      }
      function arxivOf(el, text, doi){
        var d = doi.match(/^10\.48550\/arxiv\.(.+)$/i); if (d) return d[1];
        var hay = text;
        el.querySelectorAll('a[href]').forEach(function(a){ hay += ' ' + a.getAttribute('href'); });
        var m = hay.match(/arxiv(?:\.org\/(?:abs|pdf)\/|:\s*|\s+)(\d{4}\.\d{4,5}|[a-z][a-z\-]+(?:\.[A-Z]{2})?\/\d{7})(v\d+)?/i);
        return m ? m[1] : '';
      }
      function pmidOf(el, text){
        var as = el.querySelectorAll('a[href]');
        for (var i = 0; i < as.length; i++) {
          var m = as[i].getAttribute('href').match(/(?:pubmed\.ncbi\.nlm\.nih\.gov\/|ncbi\.nlm\.nih\.gov\/pubmed\/|europepmc\.org\/(?:abstract|article)\/MED\/)(\d+)/i);
          if (m) return m[1];
        }
        var t = text.match(/\bPMID:?\s*(\d{5,9})\b/i);
        return t ? t[1] : '';
      }
      function pmcidOf(el, text){
        var hay = text;
        el.querySelectorAll('a[href]').forEach(function(a){ hay += ' ' + a.getAttribute('href'); });
        var m = hay.match(/\b(PMC\d{4,9})\b/i);
        return m ? m[1].toUpperCase() : '';
      }

      // "[12]", "12.", "12)" at the start of an entry
      var LEAD = /^\s*(?:\[(\d{1,4}[a-z]?)\]|\((\d{1,4})\)|(\d{1,4})[.)]|(\d{1,3})(?=\s+[A-ZÀ-ɏ]))\s*/;

      window.luntaReferences = function(){
        var els = items(), index = new Map();
        cache = els;
        els.forEach(function(el, i){ index.set(el, i); });
        // in-text citations: links to the entry (or into it), outside the reference list
        var cited = els.map(function(){ return 0; });
        document.body.querySelectorAll('a[href*="#"]').forEach(function(a){
          var h = a.getAttribute('href'), k = h.indexOf('#'); if (k < 0) return;
          var id = decode(h.slice(k + 1)); if (!id) return;
          var t = document.getElementById(id); if (!t) return;
          for (var e = t; e && e !== document.body; e = e.parentElement) {
            if (index.has(e)) {
              var inList = false;
              for (var p = a; p; p = p.parentElement) if (index.has(p)) { inList = true; break; }
              if (!inList) cited[index.get(e)]++;
              return;
            }
          }
        });
        var list = els.map(function(el, i){
          var c = el.cloneNode(true);
          var lab = '', l = c.querySelector('.label, .ltx_tag_bibitem');
          if (l && clean(l.textContent).length <= 40) { lab = clean(l.textContent); l.remove(); }
          c.querySelectorAll('sup.m-hl-note, .m-note').forEach(function(x){ x.remove(); });
          var text = clean(c.textContent);
          var m = text.match(LEAD);
          if (m) { if (!lab) lab = m[1] || m[2] || m[3] || m[4]; text = text.slice(m[0].length); }
          lab = lab.replace(/^\[\s*(.*?)\s*\]$/, '$1').replace(/^\(\s*(\d+)\s*\)$/, '$1').replace(/^(\d+[a-z]?)[.)]$/, '$1');
          if (!lab) {
            var par = el.parentElement;
            if (par && /^OL$/i.test(par.tagName) && /^LI$/i.test(el.tagName)) {
              var n = parseInt(par.getAttribute('start') || '1', 10) || 1;
              for (var s = el.previousElementSibling; s; s = s.previousElementSibling) if (/^LI$/i.test(s.tagName)) n++;
              lab = String(n);
            }
          }
          var full = clean(el.textContent), doi = doiOf(el, full);
          var anchor = el.id || ((el.querySelector('[id]') || {}).id) || '';
          return { index: i, label: lab, text: text, doi: doi, arxiv: arxivOf(el, full, doi), pmid: pmidOf(el, full),
                   pmcid: pmcidOf(el, full), anchor: anchor, cited: cited[i] };
        });
        return JSON.stringify(list);
      };

      var st = null;
      window.luntaShowReference = function(i){
        if (!cache || !cache[i] || !(cache[i].isConnected || cache[i].ownerDocument !== document)) cache = items();
        var el = cache[i]; if (!el) return false;
        // in the reference list's own chapter: the app opens it there (ChapterReader.swift)
        if (el.ownerDocument !== document && window.luntaShelf) { window.location.href = window.luntaShelf.href + '#lunta-ref-' + i; return true; }
        if (!st) {
          st = document.createElement('style');
          st.textContent = '.m-ref-flash { background: rgba(255, 204, 0, .35) !important; border-radius: 4px; transition: background .6s; }';
          document.documentElement.appendChild(st);
        }
        if (window.luntaMarkJump) window.luntaMarkJump();
        el.scrollIntoView({ behavior: 'smooth', block: 'center' });
        el.classList.add('m-ref-flash');
        setTimeout(function(){ el.classList.remove('m-ref-flash'); }, 1800);
        return true;
      };
    })();
    """#
}
