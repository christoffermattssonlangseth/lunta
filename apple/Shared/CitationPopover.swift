import Foundation

/// Tapping a citation ("[12]", "Smith et al., 2020") shows the reference in a small card next to
/// it instead of jumping to the reference list: the reference, "Open paper" (its DOI), "Add to
/// Lunta" (or open it, if it's already in the library) and "Show in References". Tapping outside,
/// scrolling or Esc closes it, and the reader stays where it was.
///
/// The card is attached to <html>, outside <body>, so it never becomes part of the page text that
/// highlights and reading positions are counted in. "Add to Lunta" is a lunta://cite?doi=… link,
/// handled by the app (`ExternalLinks.onLunta`), which reports back with `luntaCiteDone(text)`.
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
        '@media (prefers-color-scheme: dark) { .m-cite { box-shadow: 0 10px 30px rgba(0,0,0,.6); } .m-cite-btn { color: #9ec9d8; background: rgba(158,201,216,.14); } }';
      document.documentElement.appendChild(st);

      var pop = null;

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

      function doiOf(el){
        var a = el.querySelector('a[href*="doi.org/"]');
        if (a) { var m = a.getAttribute('href').match(/doi\.org\/(10\.[^\s?#]+)/i); if (m) { try { return decodeURIComponent(m[1]); } catch (x) { return m[1]; } } }
        var m2 = (el.textContent || '').match(/\b(10\.\d{4,9}\/[^\s"<>]+)/);
        return m2 ? m2[1].replace(/[.,;)\]]+$/, '') : '';
      }

      function close(){ if (pop) { pop.remove(); pop = null; } }
      window.luntaCiteDone = function(text){
        if (!pop) return;
        var b = pop.querySelector('.m-cite-lunta'); if (b) { b.textContent = text; b.removeAttribute('href'); }
      };

      function open(a, ref){
        close();
        var doi = doiOf(ref);
        pop = document.createElement('div'); pop.className = 'm-cite';
        var bs = getComputedStyle(document.body);
        var bg = bs.backgroundColor; if (!bg || bg === 'rgba(0, 0, 0, 0)' || bg === 'transparent') bg = getComputedStyle(document.documentElement).backgroundColor;
        pop.style.background = (bg && bg !== 'rgba(0, 0, 0, 0)') ? bg : '#fff';
        pop.style.color = bs.color;
        var body = document.createElement('div'); body.className = 'm-cite-ref';
        var copy = ref.cloneNode(true);
        copy.querySelectorAll('[id]').forEach(function(x){ x.removeAttribute('id'); });
        copy.removeAttribute && copy.removeAttribute('id');
        body.innerHTML = copy.innerHTML;
        pop.appendChild(body);
        var acts = document.createElement('div'); acts.className = 'm-cite-actions';
        function btn(label, href, cls){ var b = document.createElement('a'); b.className = 'm-cite-btn' + (cls ? ' ' + cls : ''); b.textContent = label; b.setAttribute('href', href); acts.appendChild(b); return b; }
        if (doi) {
          btn('Open paper', 'https://doi.org/' + doi);
          var l = btn('Add to Lunta', 'lunta://cite?doi=' + encodeURIComponent(doi), 'm-cite-lunta');
          l.addEventListener('click', function(){ setTimeout(function(){ if (pop && l.getAttribute('href')) l.textContent = 'Adding…'; }, 0); });
        }
        else {
          // no DOI: an arXiv number, else a search for the reference
          var ax = (ref.textContent || '').match(/arXiv:\s*(\d{4}\.\d{4,5})(v\d+)?/i);
          if (ax) {
            btn('Open paper', 'https://arxiv.org/abs/' + ax[1]);
            var la = btn('Add to Lunta', 'lunta://cite?arxiv=' + ax[1], 'm-cite-lunta');
            la.addEventListener('click', function(){ setTimeout(function(){ if (pop && la.getAttribute('href')) la.textContent = 'Adding…'; }, 0); });
          } else {
            var q = (ref.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 200);
            btn('Search', 'https://scholar.google.com/scholar?q=' + encodeURIComponent(q));
          }
        }
        var go = btn('Show in References', '#');
        go.addEventListener('click', function(ev){
          ev.preventDefault(); ev.stopPropagation(); close();
          if (ref.id) location.hash = ref.id; else ref.scrollIntoView({ block: 'center' });
        });
        pop.appendChild(acts);
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
        if (ref && !(sel && !sel.isCollapsed)) { ev.preventDefault(); ev.stopPropagation(); open(a, ref); return; }
        // a tap that only closed the card does nothing else (no page turn, no highlight menu)
        if (wasOpen) { ev.preventDefault(); ev.stopPropagation(); }
      }, true);
      window.addEventListener('scroll', close, { passive: true });
      window.addEventListener('resize', close);
      document.addEventListener('keydown', function(ev){ if (ev.key === 'Escape') close(); });
    })();
    """#
}
