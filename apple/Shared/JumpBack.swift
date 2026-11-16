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
          '#m-back button:active { background: rgba(255,255,255,.12); }';
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
        var a = ev.target.closest && ev.target.closest('a[href^="#"]');
        if (a && !(ui && ui.contains(a))) window.luntaMarkJump();
      }, true);
      // Mac and desktop: ⌘[ / Alt+← go back, like a browser
      document.addEventListener('keydown', function(ev){
        if (!shown) return;
        if (((ev.metaKey || ev.ctrlKey) && ev.key === '[') || (ev.altKey && ev.key === 'ArrowLeft')) { ev.preventDefault(); back(); }
      });
    })();
    """#
}
