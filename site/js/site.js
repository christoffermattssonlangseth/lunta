/* Lunta website interactivity & motion (self-hosted GSAP + ScrollTrigger, zero third-party requests).
   Graceful degradation: everything is readable and usable without JS, and prefers-reduced-motion disables
   all non-essential motion while keeping every interactive demo working. */
(function () {
  "use strict";

  var REDUCED = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var hasGSAP = typeof window.gsap !== "undefined";
  var hasST = hasGSAP && typeof window.ScrollTrigger !== "undefined";

  if (hasST) {
    window.gsap.registerPlugin(window.ScrollTrigger);
  }
  document.documentElement.classList.add("has-js");
  if (hasGSAP && !REDUCED) {
    document.documentElement.classList.add("has-gsap");
  }

  /* ---------- 1. Header frosted state, reading progress bar & scroll-spy ---------- */
  (function initHeader() {
    var header = document.querySelector(".site-header");
    if (!header) return;

    var bar = header.querySelector(".read-progress");
    var navLinks = Array.prototype.slice.call(header.querySelectorAll('.nav a[href^="#"]'));
    var sections = navLinks
      .map(function (a) {
        var el = document.querySelector(a.getAttribute("href"));
        return el ? { link: a, el: el } : null;
      })
      .filter(Boolean);

    function onScroll() {
      var y = window.scrollY || window.pageYOffset || 0;
      header.classList.toggle("at-top", y < 16);

      if (bar) {
        var max = Math.max(1, document.documentElement.scrollHeight - window.innerHeight);
        var pct = Math.min(1, Math.max(0, y / max));
        bar.style.transform = "scaleX(" + pct.toFixed(4) + ")";
      }

      if (sections.length) {
        var offset = y + header.offsetHeight + 120;
        var active = null;
        for (var i = 0; i < sections.length; i++) {
          if (sections[i].el.offsetTop <= offset) active = sections[i].link;
        }
        for (var j = 0; j < sections.length; j++) {
          var isCur = sections[j].link === active;
          sections[j].link.classList.toggle("is-active", isCur);
          if (isCur) sections[j].link.setAttribute("aria-current", "location");
          else sections[j].link.removeAttribute("aria-current");
        }
      }
    }

    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll, { passive: true });
  })();

  /* ---------- 2. Subtle OS platform hint on hero CTAs ---------- */
  (function initPlatformHint() {
    var ua = navigator.userAgent || "";
    var plat = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
    var isIOS = /iPhone|iPad|iPod/i.test(ua) || (/Mac/i.test(plat) && navigator.maxTouchPoints > 1);
    var isAndroid = /Android/i.test(ua);
    var isMac = /Mac/i.test(plat) && !isIOS;
    var isWin = /Win/i.test(plat);
    var isLinux = /Linux/i.test(plat) && !isAndroid;

    var appBtn = document.querySelector("[data-cta-ios]");
    var compBtn = document.querySelector("[data-cta-desktop]");
    if (!appBtn || !compBtn) return;

    function addTag(btn, text) {
      var tag = document.createElement("em");
      tag.className = "os-tag";
      tag.textContent = text;
      btn.appendChild(tag);
    }

    if (isIOS) addTag(appBtn, "Your device");
    else if (isMac) addTag(compBtn, "Mac");
    else if (isWin) addTag(compBtn, "Windows");
    else if (isLinux) addTag(compBtn, "Linux");
    else if (isAndroid) addTag(compBtn, "Android");
  })();

  /* ---------- 2b. Light / Dark theme toggle (header, all pages) ---------- */
  (function initThemeToggle() {
    var btn = document.querySelector("[data-theme-toggle]");
    var root = document.documentElement;
    var mq = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)");

    function savedTheme() {
      try { return window.localStorage.getItem("lunta-theme"); } catch (e) { return null; }
    }
    function effectiveTheme() {
      var s = savedTheme();
      if (s === "light" || s === "dark") return s;
      return mq && mq.matches ? "dark" : "light";
    }
    function apply(mode, persist) {
      if (mode === "light" || mode === "dark") {
        root.setAttribute("data-color-scheme", mode);
        if (persist) {
          try { window.localStorage.setItem("lunta-theme", mode); } catch (e) {}
        }
      }
      if (btn) {
        var isDark = effectiveTheme() === "dark";
        btn.setAttribute("aria-pressed", isDark ? "true" : "false");
        btn.setAttribute("aria-label", isDark ? "Switch to light paper theme" : "Switch to dark theme");
        var icon = btn.querySelector(".t-icon");
        if (icon) icon.textContent = isDark ? "☀" : "☾";
      }
    }

    var initial = savedTheme();
    if (initial === "light" || initial === "dark") apply(initial, false);
    else apply(null, false);

    if (btn) {
      btn.addEventListener("click", function () {
        var next = effectiveTheme() === "dark" ? "light" : "dark";
        apply(next, true);
      });
    }
  })();

  /* ---------- 2c. Sticky TOC scroll-spy & OS row highlight on subpages ---------- */
  (function initDocPages() {
    var tocLinks = Array.prototype.slice.call(document.querySelectorAll('.toc a[href^="#"]'));
    if (tocLinks.length) {
      var targets = tocLinks
        .map(function (a) {
          var el = document.querySelector(a.getAttribute("href"));
          return el ? { link: a, el: el } : null;
        })
        .filter(Boolean);

      function updateToc() {
        var y = (window.scrollY || window.pageYOffset || 0) + 140;
        var active = targets[0] && targets[0].link;
        for (var i = 0; i < targets.length; i++) {
          if (targets[i].el.getBoundingClientRect().top + (window.scrollY || 0) <= y) {
            active = targets[i].link;
          }
        }
        for (var j = 0; j < targets.length; j++) {
          var on = targets[j].link === active;
          targets[j].link.classList.toggle("is-active", on);
          if (on) targets[j].link.setAttribute("aria-current", "location");
          else targets[j].link.removeAttribute("aria-current");
        }
      }
      updateToc();
      window.addEventListener("scroll", updateToc, { passive: true });
    }

    var osRows = document.querySelectorAll("tr[data-os]");
    if (osRows.length) {
      var ua = navigator.userAgent || "";
      var plat = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
      var isIOS = /iPhone|iPad|iPod/i.test(ua) || (/Mac/i.test(plat) && navigator.maxTouchPoints > 1);
      var key = /Android/i.test(ua) ? "android"
        : (/Mac/i.test(plat) && !isIOS) ? "mac"
        : /Win/i.test(plat) ? "win"
        : /Linux/i.test(plat) ? "linux" : "";
      if (key) {
        osRows.forEach(function (tr) {
          if (tr.getAttribute("data-os") === key) {
            tr.classList.add("is-your-os");
            var firstCell = tr.querySelector("td");
            if (firstCell && !firstCell.querySelector(".os-pill")) {
              var badge = document.createElement("span");
              badge.className = "os-pill";
              badge.textContent = "Your device";
              firstCell.appendChild(document.createTextNode(" "));
              firstCell.appendChild(badge);
            }
          }
        });
      }
    }
  })();

  /* ---------- 2d. Interactive "Works with" jump pulse ---------- */
  (function initWorksLinks() {
    var links = document.querySelectorAll('.works a[href^="#"]');
    links.forEach(function (a) {
      a.addEventListener("click", function () {
        var pick = a.getAttribute("data-pick-source");
        if (pick) {
          var tab = document.querySelector('[data-source-tab="' + pick + '"]');
          if (tab) tab.click();
        }
        var target = document.querySelector(a.getAttribute("href"));
        if (target) {
          target.classList.remove("is-targeted");
          void target.offsetWidth;
          target.classList.add("is-targeted");
          setTimeout(function () { target.classList.remove("is-targeted"); }, 1400);
        }
      });
    });
  })();

  /* ---------- 2e. Interactive On-Device AI Ask demo (#ai) ---------- */
  (function initAiDemo() {
    var root = document.querySelector("[data-ai-demo]");
    if (!root) return;
    var heading = root.querySelector("[data-ai-heading]");
    var textEl = root.querySelector("[data-ai-text]");
    var footEl = root.querySelector("[data-ai-foot]");
    var btns = root.querySelectorAll("[data-ai-q]");

    var ANSWERS = {
      summary: {
        head: "Summary",
        text: "The study investigated evolving cellular processes in experimental autoimmune encephalomyelitis (EAE) by modeling disease progression and using in situ sequencing (ISS) with high-sensitivity CartaNA kit to map cell-to-cell interactions. It found that disease-associated (DA)-glia arise independently of lesions and are dynamically induced and resolved during disease progression. …",
        foot: "Made on device by Apple Intelligence. It can be wrong."
      },
      daglia: {
        head: "Answer · Results",
        text: "Disease-associated (DA)-glia — including DA-microglia, DA-astrocytes and DA-oligodendrocytes — are induced broadly across the CNS, arise independently of focal demyelinating lesions, and resolve dynamically as disease progresses.",
        foot: "Grounded in Kukanja et al., Cell 2024 · local fulltext.txt"
      },
      method: {
        head: "Answer · Methods",
        text: "The authors profiled 239 genes across mouse EAE spinal cord sections using high-sensitivity in situ sequencing (ISS, CartaNA) at baseline, onset, peak, and remission to track spatial cell-to-cell neighborhoods.",
        foot: "Grounded in Kukanja et al., Cell 2024 · local fulltext.txt"
      },
      human: {
        head: "Answer · Human MS",
        text: "In post-mortem human MS spinal cord tissue, spatially resolved profiling revealed conserved DA-glia states and active lesion rims enriched in compartmentalized glial–immune niches.",
        foot: "Grounded in Kukanja et al., Cell 2024 · local fulltext.txt"
      }
    };

    btns.forEach(function (btn) {
      btn.addEventListener("click", function () {
        var key = btn.getAttribute("data-ai-q");
        var data = ANSWERS[key];
        if (!data) return;
        btns.forEach(function (b) { b.setAttribute("aria-pressed", b === btn ? "true" : "false"); });
        if (heading) heading.textContent = data.head;
        if (textEl) textEl.textContent = data.text;
        if (footEl) footEl.textContent = data.foot;
        if (hasGSAP && !REDUCED && textEl) {
          window.gsap.fromTo(textEl, { opacity: 0.35, y: 4 }, { opacity: 1, y: 0, duration: 0.28, ease: "power2.out" });
        }
      });
    });
  })();

  /* ---------- 2f. Linked "Bring Your Library" source switcher (#switch) ---------- */
  (function initSwitchDemo() {
    var root = document.querySelector("[data-switch-demo]");
    if (!root) return;
    var tabs = root.querySelectorAll("[data-source-tab]");
    var rows = root.querySelectorAll("[data-source-row]");
    var fileEl = root.querySelector("[data-import-file]");

    var FILES = {
      zotero: "Importing Zotero.bib",
      mendeley: "Importing Mendeley.bib",
      readcube: "Importing ReadCube.bib",
      endnote: "Importing EndNote.ris"
    };

    function select(key) {
      tabs.forEach(function (b) { b.setAttribute("aria-pressed", b.getAttribute("data-source-tab") === key ? "true" : "false"); });
      rows.forEach(function (r) { r.classList.toggle("is-selected", r.getAttribute("data-source-row") === key); });
      if (fileEl && FILES[key]) {
        fileEl.textContent = FILES[key];
        if (hasGSAP && !REDUCED) {
          window.gsap.fromTo(fileEl, { opacity: 0.35 }, { opacity: 1, duration: 0.25 });
        }
      }
    }

    tabs.forEach(function (btn) {
      btn.addEventListener("click", function () {
        select(btn.getAttribute("data-source-tab"));
      });
    });
  })();

  /* ---------- 3. Ambient luminous canvas in Hero (lightweight 2D, paused offscreen) ---------- */
  (function initHeroCanvas() {
    if (REDUCED) return;
    var hero = document.querySelector(".hero");
    var canvas = hero && hero.querySelector(".hero-canvas");
    if (!hero || !canvas || !canvas.getContext) return;

    var ctx = canvas.getContext("2d");
    var dpr = Math.min(window.devicePixelRatio || 1, 2);
    var w = 0, h = 0, raf = 0, visible = true;
    var pointer = { x: -9999, y: -9999, active: false };
    var nodes = [];
    var COUNT = 34;

    function resize() {
      var rect = hero.getBoundingClientRect();
      w = Math.max(1, Math.floor(rect.width));
      h = Math.max(1, Math.floor(rect.height));
      canvas.width = Math.floor(w * dpr);
      canvas.height = Math.floor(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      if (!nodes.length) {
        for (var i = 0; i < COUNT; i++) {
          nodes.push({
            x: Math.random() * w,
            y: Math.random() * h,
            vx: (Math.random() - 0.5) * 0.24,
            vy: (Math.random() - 0.5) * 0.2,
            r: 1.2 + Math.random() * 1.8,
            amber: i % 5 === 0
          });
        }
      }
    }

    function draw() {
      if (!visible) {
        raf = 0;
        return;
      }
      ctx.clearRect(0, 0, w, h);

      if (pointer.active) {
        var g = ctx.createRadialGradient(pointer.x, pointer.y, 0, pointer.x, pointer.y, 260);
        g.addColorStop(0, "rgba(242, 180, 90, 0.09)");
        g.addColorStop(0.5, "rgba(138, 198, 214, 0.04)");
        g.addColorStop(1, "rgba(138, 198, 214, 0)");
        ctx.fillStyle = g;
        ctx.fillRect(0, 0, w, h);
      }

      for (var i = 0; i < nodes.length; i++) {
        var n = nodes[i];
        n.x += n.vx;
        n.y += n.vy;
        if (n.x < -20) n.x = w + 20;
        if (n.x > w + 20) n.x = -20;
        if (n.y < -20) n.y = h + 20;
        if (n.y > h + 20) n.y = -20;

        if (pointer.active) {
          var dxp = pointer.x - n.x;
          var dyp = pointer.y - n.y;
          var dp = Math.hypot(dxp, dyp);
          if (dp > 1 && dp < 180) {
            n.x += (dxp / dp) * 0.14;
            n.y += (dyp / dp) * 0.14;
          }
        }

        for (var j = i + 1; j < nodes.length; j++) {
          var m = nodes[j];
          var d = Math.hypot(n.x - m.x, n.y - m.y);
          if (d < 130) {
            var alpha = (1 - d / 130) * 0.11;
            ctx.strokeStyle = n.amber || m.amber
              ? "rgba(242, 180, 90, " + alpha.toFixed(3) + ")"
              : "rgba(208, 222, 224, " + alpha.toFixed(3) + ")";
            ctx.lineWidth = 1;
            ctx.beginPath();
            ctx.moveTo(n.x, n.y);
            ctx.lineTo(m.x, m.y);
            ctx.stroke();
          }
        }

        ctx.fillStyle = n.amber ? "rgba(242, 180, 90, 0.55)" : "rgba(208, 222, 224, 0.34)";
        ctx.beginPath();
        ctx.arc(n.x, n.y, n.r, 0, Math.PI * 2);
        ctx.fill();
      }

      raf = window.requestAnimationFrame(draw);
    }

    hero.addEventListener("pointermove", function (e) {
      var rect = hero.getBoundingClientRect();
      pointer.x = e.clientX - rect.left;
      pointer.y = e.clientY - rect.top;
      pointer.active = true;
    }, { passive: true });
    hero.addEventListener("pointerleave", function () {
      pointer.active = false;
    });

    if ("IntersectionObserver" in window) {
      var io = new IntersectionObserver(function (entries) {
        visible = entries[0].isIntersecting;
        if (visible && !raf) raf = window.requestAnimationFrame(draw);
      }, { threshold: 0.01 });
      io.observe(hero);
    }

    resize();
    window.addEventListener("resize", resize, { passive: true });
    raf = window.requestAnimationFrame(draw);
  })();

  /* ---------- 4. Interactive Spotlight on Bento & Platform Cards ---------- */
  (function initSpotlight() {
    if (REDUCED || window.matchMedia("(pointer: coarse)").matches) return;
    var cards = document.querySelectorAll(".cell, .plat, .step");
    cards.forEach(function (card) {
      card.addEventListener("pointermove", function (e) {
        var r = card.getBoundingClientRect();
        card.style.setProperty("--mx", (e.clientX - r.left).toFixed(1) + "px");
        card.style.setProperty("--my", (e.clientY - r.top).toFixed(1) + "px");
      }, { passive: true });
    });
  })();

  /* ---------- 5. The drawn phones' reading views: the showcase's iPhone and the Platforms' Android ----------
     Each [data-reader] screen has tappable citations (a popover on the iPhone, a bottom sheet on Android), pages
     turned by tapping the left or right third of the text, a horizontal swipe, the arrow keys or the indicator's
     buttons, and on the iPhone Fig. 1 and the theme buttons. Without JS the phones show page 1. */
  /* The paper's references 1-10 (Kukanja et al., Cell 2024), as its reference list has them */
  var READ_REFS = {
    "1": { title: "Interactions between genetic, lifestyle and environmental risk factors for multiple sclerosis", meta: "Olsson T, Barcellos LF, Alfredsson L · Nat Rev Neurol (2016)" },
    "2": { title: "Longitudinal analysis reveals high prevalence of Epstein-Barr virus associated with multiple sclerosis", meta: "Bjornevik K et al. · Science (2022)" },
    "3": { title: "Multiple sclerosis pathology", meta: "Lassmann H · Cold Spring Harb Perspect Med (2018)" },
    "4": { title: "Inflammation, demyelination, neurodegeneration and neuroprotection in the pathogenesis of multiple sclerosis", meta: "Peterson LK, Fujinami RS · J Neuroimmunol (2007)" },
    "5": { title: "Immunopathology of multiple sclerosis", meta: "Dendrou CA, Fugger L, Friese MA · Nat Rev Immunol (2015)" },
    "6": { title: "Defining the clinical course of multiple sclerosis: the 2013 revisions", meta: "Lublin FD et al. · Neurology (2014)" },
    "7": { title: "Spinal MRI in patients with suspected multiple sclerosis and negative brain MRI", meta: "Thorpe JW et al. · Brain (1996)" },
    "8": { title: "Staging of multiple sclerosis (MS) lesions: pathology of the time frame of MS", meta: "van der Valk P, De Groot CJ · Neuropathol Appl Neurobiol (2000)" },
    "9": { title: "An updated histological classification system for multiple sclerosis lesions", meta: "Kuhlmann T et al. · Acta Neuropathol (2017)" },
    "10": { title: "Pathogenic mechanisms associated with different clinical courses of multiple sclerosis", meta: "Lassmann H · Front Immunol (2019)" }
  };

  /* A short entrance: GSAP, else the Web Animations API; none with reduced motion */
  function animIn(el, from, to, ms) {
    if (REDUCED || !el) return;
    if (hasGSAP) {
      var vars = { duration: ms / 1000, ease: "power2.out", clearProps: "transform,opacity" };
      for (var k in to) vars[k] = to[k];
      window.gsap.fromTo(el, from, vars);
    } else if (el.animate) {
      var tf = function (v) { return (v.yPercent != null ? "translateY(" + v.yPercent + "%) " : "") + (v.xPercent != null ? "translateX(" + v.xPercent + "%) " : "") + (v.y != null ? "translateY(" + v.y + "px) " : "") + (v.scale != null ? "scale(" + v.scale + ")" : ""); };
      el.animate([{ transform: tf(from) || "none", opacity: from.opacity == null ? 1 : from.opacity }, { transform: tf(to) || "none", opacity: to.opacity == null ? 1 : to.opacity }], { duration: ms, easing: "cubic-bezier(.2,.7,.2,1)" });
    }
  }

  function initReader(screen, opts) {
    var popover = screen.querySelector("[data-cite-popover]");
    var popNum = popover && popover.querySelector("[data-pop-num]");
    var popRefs = popover && popover.querySelector("[data-pop-refs]");
    var popAdd = popover && popover.querySelector("[data-pop-add]");
    var popClose = popover && popover.querySelector("[data-pop-close]");
    var figOverlay = screen.querySelector("[data-fig-overlay]");
    var figClose = screen.querySelector("[data-fig-close]");
    var pagesWrap = screen.querySelector("[data-pages]");
    var ind = screen.querySelector("[data-page-ind]");
    var indNow = ind && ind.querySelector("[data-page-now]");
    var indCount = ind && ind.querySelector("[data-page-count]");
    var prevBtn = ind && ind.querySelector("[data-page-prev]");
    var nextBtn = ind && ind.querySelector("[data-page-next]");
    var markBar = screen.querySelector("[data-mark-bar]");
    var swatches = markBar ? markBar.querySelectorAll("[data-mark-color]") : [];
    if (!pagesWrap) return;
    /* the text as written in the page: one flow, cut into pages that fill the screen (below) */
    var flow = pagesWrap.querySelector("[data-page]");
    var pristine = flow.cloneNode(true);
    var pages = [flow];
    var page = 0;
    var saved = {};
    var activeRef = null;
    var activeBtn = null;
    var activeMark = null;
    var swipedAt = 0;

    function justSwiped() { return Date.now() - swipedAt < 450; }

    /* ---- pages: split the flow where a line no longer fits, at a space, never inside a highlight or a citation ---- */
    function inInline(node) {
      var el = node.parentNode;
      return !!(el && el.closest && el.closest("mark, button"));
    }
    function splitPoint(pageEl, limit) {
      var kids = pageEl.children;
      for (var k = 0; k < kids.length; k++) {
        var block = kids[k];
        if (block.getBoundingClientRect().bottom <= limit + 0.5) continue;
        if (block.tagName !== "P") return k === 0 ? null : { before: block };
        /* the first character below the limit starts the first line that does not fit */
        var walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
        var r = document.createRange();
        var best = null, node;
        while ((node = walker.nextNode())) {
          var d = node.data;
          for (var i = 0; i < d.length; i++) {
            if (i > 0 && /\s/.test(d[i - 1]) && !/\s/.test(d[i]) && !inInline(node)) best = { node: node, offset: i };
            if (/\s/.test(d[i])) continue;
            r.setStart(node, i);
            r.setEnd(node, i + 1);
            var rects = r.getClientRects();
            var rc = rects[rects.length - 1];
            if (rc && rc.bottom > limit + 0.5) {
              if (best && best.node === node && best.offset === i) return best;
              /* mid-word (a hyphenated line) or inside a highlight: break at the last space before it */
              if (best) return best;
              return k === 0 ? null : { before: block };
            }
            /* a space ending a text node before a highlight is a break too */
          }
          if (d.length && /\s/.test(d[d.length - 1]) && !inInline(node)) best = { node: node, offset: d.length };
        }
        return null;
      }
      return null;
    }

    function paginate() {
      closePopover();
      closeMarkBar();
      var keep = page;
      pagesWrap.textContent = "";
      var cur = pristine.cloneNode(true);
      cur.hidden = false;
      pagesWrap.appendChild(cur);
      pages = [cur];
      var limit = pagesWrap.getBoundingClientRect().bottom;
      for (var guard = 0; guard < 12; guard++) {
        var pt = splitPoint(cur, limit);
        if (!pt) break;
        var range = document.createRange();
        if (pt.before) range.setStartBefore(pt.before);
        else range.setStart(pt.node, pt.offset);
        range.setEndAfter(cur.lastChild);
        var rest = range.extractContents();
        var next = cur.cloneNode(false);
        next.appendChild(rest);
        /* drop what the cut left empty */
        Array.prototype.slice.call(cur.children).concat(Array.prototype.slice.call(next.children)).forEach(function (el) {
          if (el.tagName === "P" && !el.textContent.trim()) el.remove();
        });
        cur.hidden = true;
        pagesWrap.appendChild(next);
        pages.push(next);
        cur = next;
        limit = pagesWrap.getBoundingClientRect().bottom;
      }
      pages.forEach(function (p, i) {
        p.hidden = i !== 0;
        if (i > 0) p.classList.add("is-cont");
      });
      pagesWrap.querySelectorAll("[data-mark]").forEach(function (m) {
        m.setAttribute("role", "button");
        m.setAttribute("tabindex", "0");
        m.setAttribute("aria-expanded", "false");
        m.setAttribute("aria-description", "Highlight: change its colour");
      });
      page = 0;
      show(Math.min(keep, pages.length - 1), 0);
    }

    function show(i, dir) {
      pages.forEach(function (p, j) { p.hidden = j !== i; });
      page = i;
      if (indNow) indNow.textContent = String(i + 1);
      if (indCount) indCount.textContent = String(pages.length);
      if (prevBtn) prevBtn.disabled = i === 0;
      if (nextBtn) nextBtn.disabled = i === pages.length - 1;
      if (dir) animIn(pages[i], { xPercent: dir * 28, opacity: 0 }, { xPercent: 0, opacity: 1 }, 300);
    }

    function goTo(i) {
      if (i < 0 || i >= pages.length || i === page) return;
      closePopover();
      closeMarkBar();
      show(i, i > page ? 1 : -1);
    }

    /* ---- citations ---- */
    function closePopover(returnFocus) {
      if (!popover || popover.hidden) return;
      popover.hidden = true;
      screen.querySelectorAll("[data-cite-ref]").forEach(function (b) { b.setAttribute("aria-expanded", "false"); });
      if (returnFocus && activeBtn && activeBtn.isConnected) activeBtn.focus();
      activeRef = null;
      activeBtn = null;
    }

    function updateAddBtn() {
      if (!popAdd || !activeRef) return;
      var isSaved = !!saved[activeRef];
      popAdd.textContent = isSaved ? "✓ In library" : "+ Add to library";
      popAdd.classList.toggle("is-saved", isSaved);
      popAdd.setAttribute("aria-pressed", isSaved ? "true" : "false");
    }

    function openCite(btn) {
      if (!popover) return;
      if (activeBtn === btn && !popover.hidden) { closePopover(); return; }
      var ids = btn.getAttribute("data-cite-ref").split(",").filter(function (n) { return READ_REFS[n]; });
      if (!ids.length) return;
      closeMarkBar();
      activeRef = btn.getAttribute("data-cite-ref");
      activeBtn = btn;
      popNum.textContent = (ids.length > 1 ? "Refs. " : "Ref. ") + btn.textContent.replace(/,/g, ", ");
      popRefs.textContent = "";
      ids.forEach(function (n) {
        var s = document.createElement("strong");
        s.textContent = READ_REFS[n].title;
        var m = document.createElement("small");
        m.textContent = READ_REFS[n].meta;
        popRefs.appendChild(s);
        popRefs.appendChild(m);
      });
      updateAddBtn();
      popover.hidden = false;
      screen.querySelectorAll("[data-cite-ref]").forEach(function (b) { b.setAttribute("aria-expanded", b === btn ? "true" : "false"); });
      if (opts.sheet) animIn(popover, { yPercent: 100 }, { yPercent: 0 }, 280);
      else animIn(popover, { y: 10, opacity: 0 }, { y: 0, opacity: 1 }, 240);
    }

    if (popAdd) {
      popAdd.addEventListener("click", function () {
        if (!activeRef) return;
        saved[activeRef] = !saved[activeRef];
        updateAddBtn();
      });
    }
    if (popClose) popClose.addEventListener("click", function (e) { e.stopPropagation(); closePopover(true); });

    /* ---- Fig. 1 ---- */
    var figFrom = null;
    function openFig(btn) {
      if (!figOverlay) return;
      closePopover();
      closeMarkBar();
      figFrom = btn;
      figOverlay.hidden = false;
      animIn(figOverlay, { opacity: 0, scale: 0.96 }, { opacity: 1, scale: 1 }, 260);
      if (figClose) figClose.focus();
    }
    function closeFig() {
      if (!figOverlay || figOverlay.hidden) return;
      figOverlay.hidden = true;
      if (figFrom && figFrom.isConnected) figFrom.focus();
    }
    if (figClose) figClose.addEventListener("click", function (e) { e.stopPropagation(); closeFig(); });

    /* ---- the highlights: a colour bar, as in the app ---- */
    function closeMarkBar() {
      if (!markBar || markBar.hidden) return;
      markBar.hidden = true;
      if (activeMark) activeMark.setAttribute("aria-expanded", "false");
    }
    function openMarkBar(mark) {
      if (!markBar) return;
      var wasOpen = !markBar.hidden && activeMark === mark;
      closeMarkBar();
      if (wasOpen) return;
      closePopover();
      activeMark = mark;
      var color = mark.getAttribute("data-color") || "yellow";
      swatches.forEach(function (x) { x.setAttribute("aria-pressed", x.getAttribute("data-mark-color") === color ? "true" : "false"); });
      var sr = screen.getBoundingClientRect();
      var scale = sr.height / screen.offsetHeight || 1;
      var first = mark.getClientRects()[0] || mark.getBoundingClientRect();
      markBar.hidden = false;
      markBar.style.top = Math.max(4, (first.top - sr.top) / scale - markBar.offsetHeight - 6) + "px";
      mark.setAttribute("aria-expanded", "true");
      animIn(markBar, { y: -4, opacity: 0 }, { y: 0, opacity: 1 }, 180);
    }
    swatches.forEach(function (b) {
      b.addEventListener("click", function (e) {
        e.stopPropagation();
        if (!activeMark) return;
        var color = b.getAttribute("data-mark-color");
        activeMark.setAttribute("data-color", color);
        /* and in the flow, so the colour stays if the pages are cut again */
        var idx = Array.prototype.indexOf.call(pagesWrap.querySelectorAll("[data-mark]"), activeMark);
        var orig = pristine.querySelectorAll("[data-mark]")[idx];
        if (orig) orig.setAttribute("data-color", color);
        swatches.forEach(function (x) { x.setAttribute("aria-pressed", x === b ? "true" : "false"); });
      });
    });

    /* ---- taps: a citation, a highlight or Fig. 1 opens; else what is open closes; else the left or right third turns ---- */
    screen.addEventListener("click", function (e) {
      if (justSwiped()) return;
      var t = e.target;
      var inPages = pagesWrap.contains(t);
      var cite = inPages && t.closest("[data-cite-ref]");
      var mark = inPages && t.closest("[data-mark]");
      var fig = inPages && t.closest("[data-fig-open]");
      /* a tap beside an open card or colour bar closes it first (another citation or highlight opens its own) */
      if (popover && !popover.hidden && !popover.contains(t) && !cite) { closePopover(); return; }
      if (markBar && !markBar.hidden && !markBar.contains(t) && !mark) { closeMarkBar(); return; }
      if (cite) { openCite(cite); return; }
      if (mark) { openMarkBar(mark); return; }
      if (fig) { openFig(fig); return; }
      if (pages.length < 2 || !inPages || t.closest("button, a")) return;
      var r = pagesWrap.getBoundingClientRect();
      var x = (e.clientX - r.left) / r.width;
      if (x > 2 / 3) goTo(page + 1);
      else if (x < 1 / 3) goTo(page - 1);
    });

    screen.addEventListener("keydown", function (e) {
      if (e.key === "Escape") {
        if (figOverlay && !figOverlay.hidden) closeFig();
        else if (popover && !popover.hidden) closePopover(true);
        else if (markBar && !markBar.hidden) { closeMarkBar(); if (activeMark && activeMark.isConnected) activeMark.focus(); }
        return;
      }
      if ((e.key === "Enter" || e.key === " ") && e.target.matches && e.target.matches("[data-mark]")) {
        e.preventDefault();
        openMarkBar(e.target);
        return;
      }
      if (pages.length < 2 || (figOverlay && !figOverlay.hidden)) return;
      if (e.key === "ArrowRight") { e.preventDefault(); goTo(page + 1); }
      else if (e.key === "ArrowLeft") { e.preventDefault(); goTo(page - 1); }
    });

    /* swipes */
    var startX = null, startY = 0;
    pagesWrap.addEventListener("pointerdown", function (e) {
      if (e.pointerType === "mouse" && e.button !== 0) return;
      startX = e.clientX;
      startY = e.clientY;
    });
    pagesWrap.addEventListener("pointerup", function (e) {
      if (startX === null) return;
      var dx = e.clientX - startX, dy = e.clientY - startY;
      startX = null;
      if (pages.length > 1 && Math.abs(dx) > 40 && Math.abs(dx) > Math.abs(dy) * 1.2) {
        swipedAt = Date.now();
        goTo(page + (dx < 0 ? 1 : -1));
      }
    });
    pagesWrap.addEventListener("pointercancel", function () { startX = null; });

    if (prevBtn) prevBtn.addEventListener("click", function (e) { e.stopPropagation(); goTo(page - 1); });
    if (nextBtn) nextBtn.addEventListener("click", function (e) { e.stopPropagation(); goTo(page + 1); });

    paginate();
    screen.classList.add("is-paged");
    if (!screen.hasAttribute("tabindex")) screen.setAttribute("tabindex", "0");
    if (ind) ind.hidden = false;
    var hint = screen.closest("figure") && screen.closest("figure").querySelector("[data-reader-hint]");
    if (hint) hint.hidden = false;

    /* cut again when the text's size changes (another window size, or a font that arrived late) */
    var lastW = pagesWrap.offsetWidth, lastH = pagesWrap.offsetHeight, timer = null;
    function recheck() {
      if (pagesWrap.offsetWidth === lastW && pagesWrap.offsetHeight === lastH) return;
      lastW = pagesWrap.offsetWidth;
      lastH = pagesWrap.offsetHeight;
      paginate();
    }
    window.addEventListener("resize", function () {
      clearTimeout(timer);
      timer = setTimeout(recheck, 150);
    });
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(paginate);
  }

  (function initReaders() {
    document.querySelectorAll("[data-reader]").forEach(function (screen) {
      initReader(screen, { sheet: screen.classList.contains("a-screen") });
    });

    /* the showcase's theme buttons (beside its phone) */
    var phone = document.querySelector("[data-showcase-phone]");
    var screen = phone && phone.querySelector(".screen.read");
    var themeBtns = document.querySelectorAll("[data-read-theme]");
    if (!screen) return;
    themeBtns.forEach(function (btn) {
      btn.addEventListener("click", function () {
        screen.setAttribute("data-theme", btn.getAttribute("data-read-theme"));
        themeBtns.forEach(function (b) {
          b.setAttribute("aria-pressed", b === btn ? "true" : "false");
        });
      });
    });
  })();

  /* ---------- 6. Interactive Highlight Swatches + Note (#highlight) ---------- */
  (function initHighlightDemo() {
    var root = document.querySelector("[data-hl-demo]");
    if (!root) return;
    var mark = root.querySelector("mark");
    var swatches = root.querySelectorAll("[data-hl-color]");
    var noteBtn = root.querySelector("[data-hl-note-btn]");
    var noteBox = root.querySelector("[data-hl-note]");

    swatches.forEach(function (btn) {
      btn.addEventListener("click", function () {
        var color = btn.getAttribute("data-hl-color");
        mark.setAttribute("data-color", color);
        swatches.forEach(function (b) {
          b.setAttribute("aria-pressed", b === btn ? "true" : "false");
        });
        if (!REDUCED) {
          mark.classList.remove("is-inking");
          void mark.offsetWidth;
          mark.classList.add("is-inking");
        }
      });
    });

    if (noteBtn && noteBox) {
      noteBtn.addEventListener("click", function () {
        var open = noteBox.hidden;
        noteBox.hidden = !open;
        noteBtn.setAttribute("aria-expanded", open ? "true" : "false");
        noteBtn.textContent = open ? "Hide note" : "+ note";
        if (open && hasGSAP && !REDUCED) {
          window.gsap.fromTo(noteBox, { y: -6, opacity: 0 }, { y: 0, opacity: 1, duration: 0.25, ease: "power2.out" });
        }
      });
    }
  })();

  /* ---------- 7. Interactive Citation Style Switcher (#cite) ---------- */
  var CITE_STYLES = {
    nature: {
      inline: "Spatial omics maps cells in their tissue context<sup>1,2</sup>.",
      listTag: "ol",
      entries: [
        "Kukanja, P. et al. Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology. <i>Cell</i> <b>187</b>, 1990–2009 (2024).",
        "Tejada-Lapuerta, A. et al. Nicheformer: a foundation model for single-cell and spatial omics. <i>Nature Methods</i> <b>22</b>, 2525–2538 (2025)."
      ]
    },
    apa: {
      inline: "Spatial omics maps cells in their tissue context (Kukanja et al., 2024; Tejada-Lapuerta et al., 2025).",
      listTag: "ul",
      entries: [
        "Kukanja, P., Langseth, C. M., et al. (2024). Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology. <i>Cell</i>, <i>187</i>(8), 1990–2009.",
        "Tejada-Lapuerta, A., et al. (2025). Nicheformer: A foundation model for single-cell and spatial omics. <i>Nature Methods</i>, <i>22</i>, 2525–2538."
      ]
    },
    vancouver: {
      inline: "Spatial omics maps cells in their tissue context (1,2).",
      listTag: "ol",
      entries: [
        "Kukanja P, Langseth CM, et al. Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology. Cell. 2024;187(8):1990–2009.",
        "Tejada-Lapuerta A, et al. Nicheformer: a foundation model for single-cell and spatial omics. Nat Methods. 2025;22:2525–38."
      ]
    },
    cell: {
      inline: "Spatial omics maps cells in their tissue context (Kukanja et al., 2024; Tejada-Lapuerta et al., 2025).",
      listTag: "ul",
      entries: [
        "Kukanja, P., Langseth, C.M., et al. (2024). Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology. Cell <i>187</i>, 1990–2009.",
        "Tejada-Lapuerta, A., et al. (2025). Nicheformer: a foundation model for single-cell and spatial omics. Nat. Methods <i>22</i>, 2525–2538."
      ]
    },
    ieee: {
      inline: "Spatial omics maps cells in their tissue context [1], [2].",
      listTag: "ol",
      entries: [
        "P. Kukanja et al., “Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology,” <i>Cell</i>, vol. 187, pp. 1990–2009, 2024.",
        "A. Tejada-Lapuerta et al., “Nicheformer: a foundation model for single-cell and spatial omics,” <i>Nat. Methods</i>, vol. 22, pp. 2525–2538, 2025."
      ]
    },
    bibtex: {
      inline: "Spatial omics maps cells in their tissue context <code>\\cite{kukanja2024,tejada2025}</code>.",
      listTag: "pre",
      entries: [
        "@article{kukanja2024,\n  author  = {Kukanja, Petra and others},\n  title   = {Cellular architecture of evolving neuroinflammatory lesions},\n  journal = {Cell}, volume = {187}, year = {2024}\n}"
      ]
    }
  };

  (function initCiteDemo() {
    var root = document.querySelector("[data-cite-demo]");
    if (!root) return;
    var btns = root.querySelectorAll("[data-cite-style]");
    var page = root.querySelector("[data-cite-page]");
    if (!page || !btns.length) return;

    function render(key) {
      var data = CITE_STYLES[key];
      if (!data) return;
      var html = "<p>" + data.inline + "</p>";
      if (data.listTag === "pre") {
        html += '<pre class="cite-bib">' + data.entries[0] + "</pre>";
      } else {
        var cls = data.listTag === "ul" ? ' class="plain-refs"' : "";
        html += "<" + data.listTag + cls + ">" + data.entries.map(function (e) { return "<li>" + e + "</li>"; }).join("") + "</" + data.listTag + ">";
      }
      page.innerHTML = html;
      btns.forEach(function (b) {
        b.setAttribute("aria-pressed", b.getAttribute("data-cite-style") === key ? "true" : "false");
      });
      if (hasGSAP && !REDUCED) {
        window.gsap.fromTo(page, { opacity: 0.45, y: 4 }, { opacity: 1, y: 0, duration: 0.25, ease: "power2.out" });
      }
    }

    btns.forEach(function (btn) {
      btn.addEventListener("click", function () {
        render(btn.getAttribute("data-cite-style"));
      });
    });
  })();

  /* ---------- 8. Interactive Listen Waveform (click-to-seek + clickable sentences) ---------- */
  (function initListen() {
    var r = document.querySelector("[data-listen]");
    if (!r) return;
    var a = r.querySelector("audio");
    var b = r.querySelector(".play");
    var barsWrap = r.querySelector(".bars");
    var bars = r.querySelectorAll(".bars i");
    var ss = r.querySelectorAll("[data-t]");
    var DEFAULT_DUR = 27.6;

    a.controls = false;
    a.hidden = true;
    b.hidden = false;
    r.classList.add("js");

    function tick() {
      var t = a.currentTime;
      var d = a.duration || DEFAULT_DUR;
      var n = a.ended ? 0 : Math.round((t / d) * bars.length);
      var k = -1;
      for (var i = 0; i < bars.length; i++) {
        bars[i].classList.toggle("done", i < n);
        bars[i].classList.toggle("head", !a.paused && !a.ended && i === Math.max(0, n - 1));
      }
      if (!a.ended && t > 0) {
        for (var j = 0; j < ss.length; j++) {
          if (t >= +ss[j].dataset.t) k = j;
        }
      }
      for (var m = 0; m < ss.length; m++) {
        ss[m].classList.toggle("now", m === k);
      }
    }

    function state() {
      var p = !a.paused && !a.ended;
      b.setAttribute("aria-pressed", p);
      b.classList.toggle("on", p);
      r.classList.toggle("is-playing", p);
      tick();
    }

    function seekToRatio(ratio) {
      var clamped = Math.max(0, Math.min(0.995, ratio));
      a.preload = "auto";
      var go = function () {
        var dur = a.duration || DEFAULT_DUR;
        a.currentTime = clamped * dur;
        a.play().catch(function () {});
        tick();
      };
      if (a.readyState >= 1) go();
      else {
        a.addEventListener("loadedmetadata", go, { once: true });
        a.load();
      }
    }

    b.addEventListener("click", function () {
      if (a.paused) {
        a.preload = "auto";
        a.play().catch(function () {});
      } else {
        a.pause();
      }
    });

    if (barsWrap) {
      barsWrap.addEventListener("click", function (e) {
        var rect = barsWrap.getBoundingClientRect();
        seekToRatio((e.clientX - rect.left) / Math.max(1, rect.width));
      });
    }

    ss.forEach(function (span) {
      span.setAttribute("role", "button");
      span.setAttribute("tabindex", "0");
      var jump = function () {
        var t = parseFloat(span.dataset.t || "0");
        var dur = a.duration || DEFAULT_DUR;
        seekToRatio(t / dur);
      };
      span.addEventListener("click", jump);
      span.addEventListener("keydown", function (e) {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          jump();
        }
      });
    });

    ["play", "pause", "ended", "timeupdate", "seeked"].forEach(function (ev) {
      a.addEventListener(ev, ev === "timeupdate" ? tick : state);
    });
  })();

  /* ---------- 9. Interactive AI Agents (MCP) Chips & Copy Button (#agents) ---------- */
  (function initAgentDemo() {
    var root = document.querySelector("[data-agent-demo]");
    if (!root) return;
    var chips = root.querySelectorAll("[data-mcp-tool]");
    var logOut = root.querySelector("[data-agent-output]");
    var copyBtn = root.querySelector("[data-copy-mcp]");

    var TOOLS = {
      search_papers: {
        cmd: 'search_papers({ query: "disease-associated glia spatial" })',
        res: "Found 4 papers in your library · Kukanja et al. (Cell, 2024) matched 11 passages",
        undoable: false
      },
      read_paper: {
        cmd: 'read_paper({ id: "kukanja2024", query: "DA-glia" })',
        res: "Returned 3 passages from Results & Discussion (1,140 words, local fulltext.txt)",
        undoable: false
      },
      add_paper: {
        cmd: 'add_paper({ doi: "10.1016/j.cell.2024.02.030" })',
        res: "Added Kukanja et al. (2024) · logged in ops/agent-claude.jsonl",
        undoable: true
      },
      find_pdf: {
        cmd: 'find_pdf({ id: "kukanja2024" })',
        res: "Saved open-access PDF from Europe PMC (CC BY 4.0) · building reading view",
        undoable: true
      },
      add_to_collection: {
        cmd: 'add_to_collection({ id: "kukanja2024", folder: "MS Lesions" })',
        res: "Filed in “MS Lesions” · synced to iCloud Drive",
        undoable: true
      },
      add_highlight: {
        cmd: 'add_highlight({ id: "kukanja2024", color: "yellow", quote: "DA-glia arise independently…" })',
        res: "Highlighted in reading view & PDF · logged in ops/agent-claude.jsonl",
        undoable: true
      },
      cite: {
        cmd: 'cite({ ids: ["kukanja2024", "tejada2025"], style: "nature" })',
        res: "1. Kukanja, P. et al. Cell 187, 1990–2009 (2024).  2. Tejada-Lapuerta, A. et al. Nat. Methods (2025).",
        undoable: false
      }
    };

    chips.forEach(function (btn) {
      btn.addEventListener("click", function () {
        var key = btn.getAttribute("data-mcp-tool");
        var info = TOOLS[key];
        if (!info || !logOut) return;
        chips.forEach(function (b) { b.setAttribute("aria-pressed", b === btn ? "true" : "false"); });
        logOut.innerHTML =
          '<div><span>›</span> <em>' + info.cmd + "</em></div>" +
          '<div class="agent-res"><span>✓</span> ' + info.res +
          (info.undoable ? ' <button type="button" class="undo-pill" data-undo-sim>Undo</button>' : "") +
          "</div>";

        var undoBtn = logOut.querySelector("[data-undo-sim]");
        if (undoBtn) {
          undoBtn.addEventListener("click", function () {
            undoBtn.textContent = "Reverted";
            undoBtn.disabled = true;
            undoBtn.classList.add("is-done");
          });
        }

        if (hasGSAP && !REDUCED) {
          window.gsap.fromTo(logOut, { opacity: 0.35, y: 4 }, { opacity: 1, y: 0, duration: 0.24, ease: "power2.out" });
        }
      });
    });

    if (copyBtn) {
      var origText = copyBtn.textContent;
      copyBtn.addEventListener("click", function () {
        var cmd = copyBtn.getAttribute("data-copy-mcp");
        if (navigator.clipboard && navigator.clipboard.writeText) {
          navigator.clipboard.writeText(cmd).then(function () {
            copyBtn.textContent = "Copied ✓";
            setTimeout(function () { copyBtn.textContent = origText; }, 1800);
          }).catch(function () {});
        }
      });
    }
  })();

  /* ---------- 10. Smooth <details class="compare"> Accordion ---------- */
  (function initDetails() {
    var details = document.querySelector(".compare");
    if (!details || REDUCED) return;
    var summary = details.querySelector("summary");
    var body = details.querySelector(".compare-body");
    if (!summary || !body) return;

    summary.addEventListener("click", function (e) {
      if (!hasGSAP) return;
      e.preventDefault();
      if (details.open) {
        window.gsap.to(body, {
          height: 0,
          opacity: 0,
          duration: 0.28,
          ease: "power2.inOut",
          onComplete: function () {
            details.open = false;
            body.style.height = "";
            body.style.opacity = "";
          }
        });
      } else {
        details.open = true;
        var fullH = body.scrollHeight;
        window.gsap.fromTo(
          body,
          { height: 0, opacity: 0 },
          {
            height: fullH,
            opacity: 1,
            duration: 0.36,
            ease: "power2.out",
            onComplete: function () {
              body.style.height = "";
              if (hasST) window.ScrollTrigger.refresh();
            }
          }
        );
      }
    });
  })();

  /* ---------- 11. GSAP Choreography & ScrollTrigger Animations ---------- */
  if (hasGSAP && hasST && !REDUCED) {
    var gsap = window.gsap;
    var ScrollTrigger = window.ScrollTrigger;

    /* Hero entrance timeline */
    /* Movement only, no fade: the hero (title, download buttons) is visible from the first frame even where
       animations never run (link previews, screenshot tools, a stalled clock). */
    /* Only on the front page: every page loads this file, and GSAP warns about selectors that match nothing. */
    var qsa = function (sel) { return document.querySelectorAll(sel); };
    if (document.querySelector(".hero")) {
      var heroTl = gsap.timeline({ defaults: { ease: "power3.out" } });
      [
        [".hero .pill", { y: 16, duration: 0.55 }, undefined],
        [".hero h1", { y: 26, duration: 0.7 }, "-=0.35"],
        [".hero .lead", { y: 18, duration: 0.6 }, "-=0.45"],
        [".hero .cta-row .btn", { y: 16, stagger: 0.08, duration: 0.55 }, "-=0.4"],
        [".hero .cta-links, .hero .hero-meta", { y: 12, stagger: 0.08, duration: 0.5 }, "-=0.35"],
        [".hero-art .tablet", { y: 40, scale: 0.96, duration: 0.9, ease: "power2.out" }, 0.15],
        [".hero-art .phone", { y: 56, scale: 0.94, duration: 0.9, ease: "power2.out" }, 0.3]
      ].forEach(function (step) {
        var els = qsa(step[0]);
        if (els.length) heroTl.from(els, step[1], step[2]);
      });
    }

    /* Hero subtle mouse 3D depth on desktop */
    var heroArt = document.querySelector(".hero-art");
    var heroTablet = heroArt && heroArt.querySelector(".tablet");
    var heroPhone = heroArt && heroArt.querySelector(".phone");
    if (heroArt && heroTablet && heroPhone && window.matchMedia("(min-width: 62rem) and (pointer: fine)").matches) {
      var tX = gsap.quickTo(heroTablet, "x", { duration: 0.6, ease: "power2.out" });
      var tY = gsap.quickTo(heroTablet, "y", { duration: 0.6, ease: "power2.out" });
      var pX = gsap.quickTo(heroPhone, "x", { duration: 0.5, ease: "power2.out" });
      var pY = gsap.quickTo(heroPhone, "y", { duration: 0.5, ease: "power2.out" });
      var heroEl = document.querySelector(".hero");
      heroEl.addEventListener("pointermove", function (e) {
        var r = heroEl.getBoundingClientRect();
        var nx = (e.clientX - r.left) / r.width - 0.5;
        var ny = (e.clientY - r.top) / r.height - 0.5;
        tX(nx * -14);
        tY(ny * -10);
        pX(nx * 20);
        pY(ny * 14);
      }, { passive: true });
      heroEl.addEventListener("pointerleave", function () {
        tX(0); tY(0); pX(0); pY(0);
      });
    }

    /* Hero scroll parallax separation */
    if (heroTablet && heroPhone) {
      gsap.to(heroTablet, {
        yPercent: -8,
        ease: "none",
        scrollTrigger: { trigger: ".hero", start: "top top", end: "bottom top", scrub: 0.6 }
      });
      gsap.to(heroPhone, {
        yPercent: -15,
        ease: "none",
        scrollTrigger: { trigger: ".hero", start: "top top", end: "bottom top", scrub: 0.6 }
      });
    }

    /* "Works with" subtle stagger */
    var worksItems = document.querySelectorAll(".works li");
    if (worksItems.length) {
      gsap.from(worksItems, {
        y: 10,
        stagger: 0.04,
        duration: 0.45,
        ease: "power2.out",
        scrollTrigger: { trigger: ".works", start: "top 90%", once: true }
      });
    }

    /* Signature Showcase: PDF tilts/scans -> Arrow pulses -> Phone reading view locks in */
    var stage = document.querySelector(".stage");
    if (stage) {
      var stagePdf = stage.querySelector(".pdf-wrap");
      var stageArrow = stage.querySelector(".arrow");
      var stagePhone = stage.querySelector(".phone");
      var scanBeam = stage.querySelector(".pdf-scan");

      if (stagePdf) {
        gsap.fromTo(
          stagePdf,
          { rotateY: 0, rotateZ: 0, scale: 1 },
          {
            rotateY: 8,
            rotateZ: -1.8,
            scale: 0.95,
            transformPerspective: 1000,
            ease: "none",
            scrollTrigger: { trigger: stage, start: "top 75%", end: "center 40%", scrub: 0.7 }
          }
        );
      }
      if (scanBeam) {
        gsap.fromTo(
          scanBeam,
          { top: "-12%", opacity: 0 },
          {
            top: "105%",
            opacity: 0.9,
            ease: "none",
            scrollTrigger: { trigger: stage, start: "top 75%", end: "center 35%", scrub: 0.6 }
          }
        );
      }
      if (stageArrow) {
        gsap.from(stageArrow, {
          scale: 0.65,
          opacity: 0,
          duration: 0.55,
          ease: "back.out(1.7)",
          scrollTrigger: { trigger: stage, start: "top 72%", once: true }
        });
      }
      if (stagePhone) {
        gsap.from(stagePhone, {
          y: 36,
          opacity: 0,
          scale: 0.95,
          duration: 0.75,
          ease: "power3.out",
          scrollTrigger: { trigger: stage, start: "top 75%", once: true }
        });
      }
    }

    /* Benchmark Proof Counters (99% on the front page; 99%, 0.95, 97%, 89% on features.html) */
    var counters = document.querySelectorAll("[data-count-to]");
    counters.forEach(function (el) {
      var target = parseFloat(el.getAttribute("data-count-to"));
      var suffix = el.getAttribute("data-count-suffix") || "";
      var decimals = parseInt(el.getAttribute("data-count-decimals") || "0", 10);
      var meter = el.parentElement && el.parentElement.querySelector(".proof-meter i");
      var obj = { val: 0 };

      ScrollTrigger.create({
        trigger: el,
        start: "top 86%",
        once: true,
        onEnter: function () {
          gsap.to(obj, {
            val: target,
            duration: 1.15,
            ease: "power2.out",
            onUpdate: function () {
              el.textContent = obj.val.toFixed(decimals) + suffix;
            }
          });
          if (meter) {
            var pct = target <= 1 ? target * 100 : target;
            gsap.fromTo(meter, { scaleX: 0 }, { scaleX: pct / 100, duration: 1.15, ease: "power2.out" });
          }
        }
      });
    });

    /* Universal .reveal elements (works identically on Safari, iOS, Firefox, Chrome) */
    var reveals = gsap.utils.toArray(".reveal");
    reveals.forEach(function (el) {
      gsap.from(el, {
        y: 26,
        opacity: 0,
        duration: 0.68,
        ease: "power2.out",
        clearProps: "transform",
        scrollTrigger: { trigger: el, start: "top 88%", once: true }
      });
    });

    /* Highlight card initial ink sweep on scroll */
    var hlMark = document.querySelector(".hl mark");
    if (hlMark) {
      ScrollTrigger.create({
        trigger: hlMark,
        start: "top 82%",
        once: true,
        onEnter: function () {
          hlMark.classList.add("is-inking");
        }
      });
    }

    /* Import progress bar + live counter in Step 03 */
    var importBar = document.querySelector(".im-bar i");
    var importCount = document.querySelector("[data-import-count]");
    var importMock = document.querySelector(".import-mock");
    if (importBar && importCount && importMock) {
      var imObj = { n: 12 };
      ScrollTrigger.create({
        trigger: importMock,
        start: "top 84%",
        once: true,
        onEnter: function () {
          gsap.fromTo(importBar, { scaleX: 0.06 }, { scaleX: 1, duration: 1.8, ease: "power2.inOut" });
          gsap.to(imObj, {
            n: 312,
            duration: 1.8,
            ease: "power2.inOut",
            onUpdate: function () {
              var v = Math.round(imObj.n);
              importCount.textContent = v >= 312 ? "Reading views ready · 312 of 312" : "Making reading views · " + v + " of 312";
            }
          });
        }
      });
    }
  }
})();
