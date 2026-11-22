import Foundation

// "Back": after a jump far through the paper (a citation's "Show in References", a link to a
// section or a table, the contents, a highlight in the list), a small Back button appears at the
// bottom of the reader and returns you to where you were reading. It goes away when you tap it or
// its ×, or when you scroll back to that place yourself. Several jumps in a row keep the first
// place, so Back always returns to where you left the text. Works in both scroll and page modes;
// the same script runs on Mac, iPhone, iPad, Android and desktop (extract-reader-assets.mjs).

extension ReaderAssets {
    static let jumpBackJS = #"""
    (function(){
      if (window.luntaMarkJump) return;
      var from = null, armed = 0, shown = false, settle = null, ui = null;
      function pos(){ return { x: window.scrollX, y: window.scrollY }; }
      function far(a, b){
        return Math.abs(a.x - b.x) > 1.5 * window.innerWidth || Math.abs(a.y - b.y) > 1.5 * window.innerHeight;
      }
      function near(a, b){
        return Math.abs(a.x - b.x) < 0.5 * window.innerWidth && Math.abs(a.y - b.y) < 0.6 * window.innerHeight;
      }

      function build(){
        if (ui) return ui;
        var st = document.createElement('style');
        st.textContent =
          '#m-back { position: fixed; left: 50%; transform: translateX(-50%); bottom: calc(env(safe-area-inset-bottom, 0px) + 54px);' +
          ' z-index: 30; display: none; align-items: center; border-radius: 20px; overflow: hidden;' +
          ' background: rgba(44,44,48,.9); color: #fff; box-shadow: 0 2px 10px rgba(0,0,0,.25);' +
          ' font: 500 14px -apple-system, system-ui, "Segoe UI", Roboto, sans-serif; -webkit-user-select: none; user-select: none; }' +
          '#m-back.on { display: flex; }' +
          '#m-back button { all: unset; cursor: pointer; padding: 9px 14px; color: #fff; -webkit-tap-highlight-color: transparent; }' +
          '#m-back .m-back-x { padding: 9px 12px 9px 8px; opacity: .65; border-left: 1px solid rgba(255,255,255,.18); }' +
          '#m-back button:active { background: rgba(255,255,255,.12); }' +
          '#m-goto { position: fixed; left: 50%; transform: translateX(-50%); bottom: calc(env(safe-area-inset-bottom, 0px) + 54px);' +
          ' z-index: 31; width: min(340px, calc(100vw - 32px)); box-sizing: border-box; padding: 12px 14px 10px; border-radius: 14px;' +
          ' background: rgba(44,44,48,.94); color: #fff; box-shadow: 0 4px 18px rgba(0,0,0,.3);' +
          ' font: 14px -apple-system, system-ui, "Segoe UI", Roboto, sans-serif; -webkit-user-select: none; user-select: none; }' +
          '#m-goto .m-goto-row { display: flex; align-items: center; gap: 8px; }' +
          '#m-goto .m-goto-t { font-weight: 600; flex: 1; }' +
          '#m-goto .m-goto-of { opacity: .7; white-space: nowrap; }' +
          '#m-goto input.m-goto-n { width: 4.2em; padding: 4px 6px; border-radius: 6px; border: 1px solid rgba(255,255,255,.25);' +
          ' background: rgba(255,255,255,.1); color: #fff; font: inherit; text-align: right; -webkit-user-select: text; user-select: text; }' +
          '#m-goto input.m-goto-n::-webkit-inner-spin-button, #m-goto input.m-goto-n::-webkit-outer-spin-button { -webkit-appearance: none; margin: 0; }' +
          '#m-goto input.m-goto-n { -moz-appearance: textfield; appearance: textfield; }' +
          '#m-goto button { all: unset; cursor: pointer; padding: 4px 12px; border-radius: 7px; background: rgba(10,132,255,.95); color: #fff; font-weight: 600; }' +
          '#m-goto input.m-goto-s { width: 100%; margin: 12px 0 2px; accent-color: #0a84ff; }';
        document.documentElement.appendChild(st);
        ui = document.createElement('div'); ui.id = 'm-back';
        var go = document.createElement('button'); go.className = 'm-back-go'; go.textContent = '↩︎ Back';
        go.setAttribute('aria-label', 'Back to where you were reading');
        var x = document.createElement('button'); x.className = 'm-back-x'; x.textContent = '✕';
        x.setAttribute('aria-label', 'Dismiss');
        go.addEventListener('click', function(e){ e.preventDefault(); e.stopPropagation(); back(); });
        x.addEventListener('click', function(e){ e.preventDefault(); e.stopPropagation(); hide(); });
        ui.appendChild(go); ui.appendChild(x);
        // outside <body>: never part of the paper's text, its pages or its highlights
        document.documentElement.appendChild(ui);
        return ui;
      }
      function show(){ build().classList.add('on'); shown = true; }
      function hide(){ if (ui) ui.classList.remove('on'); shown = false; from = null; armed = 0; }

      function back(){
        var f = from; hide();
        if (!f) return;
        var ok = f.off != null && window.luntaScrollToOffset && window.luntaScrollToOffset(f.off);
        if (!ok) window.scrollTo({ left: f.p.x, top: f.p.y, behavior: 'instant' });
      }

      // called just before a jump: remembers the place (the first one, while Back is showing)
      window.luntaMarkJump = function(){
        if (!from) from = { p: pos(), off: window.luntaReadingOffset ? window.luntaReadingOffset() : null };
        armed = Date.now() + 2000;
        check();
      };
      window.luntaJumpBack = function(){ if (!shown) return false; back(); return true; };

      function check(){
        clearTimeout(settle);
        settle = setTimeout(function(){
          if (!from) return;
          var p = pos();
          if (shown) { if (near(p, from.p)) hide(); return; }
          if (far(p, from.p)) { if (Date.now() < armed) show(); else from = null; }
          else if (Date.now() >= armed) from = null;   // the jump didn't go far: nothing to go back to
        }, 250);
      }
      window.addEventListener('scroll', function(){ if (from) check(); }, { passive: true });

      // links inside the paper (sections, tables, references, "Show in References")
      window.addEventListener('click', function(ev){
        if (window.luntaExtending && window.luntaExtending()) return;   // a tap ending a highlight (Highlights.swift), not a jump
        var a = ev.target.closest && ev.target.closest('a[href^="#"]');
        if (a && !(ui && ui.contains(a))) window.luntaMarkJump();
      }, true);
      // ---- Go to Page ----
      // Pages as the reader shows them: in page mode the label's numbers (two columns = two pages),
      // in scroll mode one screenful each.
      window.luntaPagePosition = function(){
        var i = window.luntaPageInfo && window.luntaPageInfo();
        if (i) return i.cols > 1 ? { page: 2 * i.page + 1, pages: 2 * i.pages } : { page: i.page + 1, pages: i.pages };
        // the last page is the bottom of the paper (the view can't scroll a whole screen past it)
        var h = window.innerHeight || 1, maxY = Math.max(0, document.documentElement.scrollHeight - h);
        var total = Math.max(1, Math.ceil((maxY - 2) / h) + 1);
        return { page: window.scrollY >= maxY - 2 ? total : Math.min(total, Math.floor((window.scrollY + 2) / h) + 1), pages: total };
      };
      window.luntaGoTo = function(n){
        var p = window.luntaPagePosition();
        n = Math.max(1, Math.min(p.pages, Math.round(Number(n) || 1)));
        if (n === p.page) return n;
        window.luntaMarkJump();
        var i = window.luntaPageInfo && window.luntaPageInfo();
        if (i) window.luntaGoToPage(i.cols > 1 ? Math.floor((n - 1) / 2) : n - 1, false);
        else window.scrollTo({ left: 0, top: (n - 1) * window.innerHeight, behavior: 'instant' });
        return n;
      };

      var go = null;
      function closeGoTo(){ if (go) { go.remove(); go = null; } }
      window.luntaShowGoTo = function(){
        // already open (the menu and the page can both answer ⌥⌘G): keep it, with the field ready
        if (go) { var f = go.querySelector('.m-goto-n'); if (f && !('ontouchstart' in window)) { f.focus(); f.select(); } return; }
        build();
        var p = window.luntaPagePosition();
        go = document.createElement('div'); go.id = 'm-goto';
        go.innerHTML =
          '<div class="m-goto-row"><span class="m-goto-t">Go to page</span>' +
          '<input class="m-goto-n" type="number" inputmode="numeric" min="1"> <span class="m-goto-of"></span>' +
          '<button class="m-goto-go">Go</button></div>' +
          '<input class="m-goto-s" type="range" min="1" step="1">';
        var num = go.querySelector('.m-goto-n'), sl = go.querySelector('.m-goto-s');
        num.max = sl.max = String(p.pages); num.value = sl.value = String(p.page);
        go.querySelector('.m-goto-of').textContent = 'of ' + p.pages;
        function jump(v){ var n = window.luntaGoTo(v); num.value = sl.value = String(n); }
        sl.addEventListener('input', function(){ num.value = sl.value; });
        sl.addEventListener('change', function(){ jump(sl.value); });
        num.addEventListener('keydown', function(e){
          if (e.key === 'Enter') { e.preventDefault(); jump(num.value); closeGoTo(); }
          else if (e.key === 'Escape') { e.preventDefault(); closeGoTo(); }
          e.stopPropagation();   // typing a number doesn't turn the page
        });
        go.querySelector('.m-goto-go').addEventListener('click', function(e){ e.preventDefault(); jump(num.value); closeGoTo(); });
        ['click', 'mousedown', 'touchstart', 'pointerdown'].forEach(function(t){ go.addEventListener(t, function(e){ e.stopPropagation(); }); });
        // outside <body>, like the Back button
        document.documentElement.appendChild(go);
        // a keyboard only where there is one (on a phone it would resize the pages): the slider is enough
        if (!('ontouchstart' in window)) { num.focus(); num.select(); }
      };
      // a tap or click anywhere else closes it
      document.addEventListener('click', function(e){ if (go && !go.contains(e.target)) closeGoTo(); }, true);

      // Mac and desktop: ⌘[ / Alt+← go back, like a browser; ⌥⌘G / Ctrl+Alt+G go to a page
      document.addEventListener('keydown', function(ev){
        if ((ev.metaKey || ev.ctrlKey) && ev.altKey && (ev.key === 'g' || ev.key === 'G' || ev.code === 'KeyG')) { ev.preventDefault(); window.luntaShowGoTo(); return; }
        if (ev.key === 'Escape' && go) { closeGoTo(); return; }
        if (!shown) return;
        if (((ev.metaKey || ev.ctrlKey) && ev.key === '[') || (ev.altKey && ev.key === 'ArrowLeft')) { ev.preventDefault(); back(); }
      });
    })();
    """#
}
