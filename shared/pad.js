/* Touchpad + keyboard, written once for two transports:
   - Wi-Fi: api calls become WebSocket commands to the PC agent.
   - Bluetooth: api calls go through the Android app's HID bridge.
   api = { move(dx,dy), button(button, action), scroll(dx,dy), text(s), key(name, mods) }. */
(function () {
  'use strict';
  var KB_SENTINEL = '​​';

  function el(tag, cls, html) { var e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; }

  window.mountPad = function (root, api, opts) {
    opts = opts || {};
    root.textContent = '';
    var pad = el('div', 'pad');
    var surface = el('div', 'pad-surface', '<div class="hint">Drag to move the cursor<br>Tap to click · two fingers: tap = right click, drag = scroll</div>');
    var mouseRow = el('div', 'pad-row');
    var left = el('button', 'mouse', 'Left click'), right = el('button', 'mouse', 'Right click');
    mouseRow.append(left, right);

    var mods = { ctrl: false, alt: false, shift: false, win: false };
    var modBtns = {};
    var keyRow = el('div', 'pad-row');
    var kbBtn = el('button', 'primary small', '<svg viewBox="0 0 24 24"><path d="M20 5H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2zm-9 3h2v2h-2zm0 3h2v2h-2zM8 8h2v2H8zm0 3h2v2H8zM5 8h2v2H5zm0 3h2v2H5zm3 6H5v-2h3zm9 0H7v-2h10zm2 0h-2v-2h2zm0-4h-2v-2h2zm0-3h-2V8h2zm-3 3h-2v-2h2zm0-3h-2V8h2z"/></svg> Keyboard');
    function plain(label, name, m) {
      var b = el('button', 'small', label);
      b.onclick = function () { var all = (m || []).concat(activeMods().filter(function (x) { return (m || []).indexOf(x) < 0; })); api.key(name, all); clearMods(); };
      return b;
    }
    var moreBtn = el('button', 'small', 'More');
    keyRow.append(kbBtn, plain('Esc', 'esc'), plain('Tab', 'tab'), plain('⌫', 'backspace'), plain('↵', 'enter'), moreBtn);

    var more = el('div', 'pad-more');
    var modRow = el('div', 'pad-row');
    ['ctrl', 'alt', 'shift', 'win'].forEach(function (m) {
      var b = el('button', 'small mod', m === 'win' ? 'Win' : m[0].toUpperCase() + m.slice(1));
      b.onclick = function () { mods[m] = !mods[m]; b.classList.toggle('on', mods[m]); };
      modBtns[m] = b; modRow.append(b);
    });
    var arrowRow = el('div', 'pad-row');
    [['←', 'left'], ['↑', 'up'], ['↓', 'down'], ['→', 'right'], ['Del', 'delete'], ['Home', 'home'], ['End', 'end']].forEach(function (k) { arrowRow.append(plain(k[0], k[1])); });
    var pgRow = el('div', 'pad-row');
    [['PgUp', 'pageup'], ['PgDn', 'pagedown'], ['Alt+Tab', 'tab', ['alt']], ['Copy', 'c', ['ctrl']], ['Paste', 'v', ['ctrl']], ['Undo', 'z', ['ctrl']]].forEach(function (k) { pgRow.append(plain(k[0], k[1], k[2])); });
    more.append(modRow, arrowRow, pgRow);
    moreBtn.onclick = function () { more.classList.toggle('open'); moreBtn.classList.toggle('on'); };

    var note = el('div', 'pad-note', opts.note || '');
    var kb = el('input', 'pad-kb');
    kb.type = 'text'; kb.autocomplete = 'off'; kb.setAttribute('autocorrect', 'off'); kb.setAttribute('autocapitalize', 'off'); kb.spellcheck = false;
    kb.setAttribute('enterkeyhint', 'enter'); kb.setAttribute('aria-label', 'Keyboard input');
    pad.append(surface, mouseRow, keyRow, more, note, kb);
    root.append(pad);

    /* ---- one-shot modifiers ---- */
    function activeMods() { return Object.keys(mods).filter(function (m) { return mods[m]; }); }
    function clearMods() { Object.keys(mods).forEach(function (m) { mods[m] = false; modBtns[m].classList.remove('on'); }); }
    function withMods(fn) { var a = activeMods(); fn(a); if (a.length) clearMods(); }

    /* ---- mouse buttons: real press / release so dragging works ---- */
    function hold(btn, which) {
      var down = function (e) { e.preventDefault(); btn.classList.add('on'); api.button(which, 'down'); };
      var up = function (e) { e.preventDefault(); if (btn.classList.contains('on')) { btn.classList.remove('on'); api.button(which, 'up'); } };
      btn.addEventListener('touchstart', down, { passive: false });
      btn.addEventListener('touchend', up, { passive: false });
      btn.addEventListener('touchcancel', up, { passive: false });
      btn.addEventListener('mousedown', down); btn.addEventListener('mouseup', up); btn.addEventListener('mouseleave', up);
    }
    hold(left, 'left'); hold(right, 'right');

    /* ---- touch surface ---- */
    var st = { fingers: 0, startT: 0, startX: 0, startY: 0, lastX: 0, lastY: 0, lastT: 0, moved: 0, maxFingers: 0, dragging: false, dragArmed: false, lastTapEnd: 0, remX: 0, remY: 0, cx: 0, cy: 0, sx: 0, sy: 0 };
    var pending = { dx: 0, dy: 0, sx: 0, sy: 0 }, timer = null;

    function flush() {
      timer = null;
      var dx = Math.trunc(pending.dx), dy = Math.trunc(pending.dy);
      if (dx || dy) { pending.dx -= dx; pending.dy -= dy; api.move(dx, dy); }
      var sy = Math.trunc(pending.sy), sx = Math.trunc(pending.sx);
      if (sx || sy) { pending.sy -= sy; pending.sx -= sx; api.scroll(sx, sy); }
    }
    function schedule() { if (!timer) timer = setTimeout(flush, 16); }

    function centroid(ts) {
      var x = 0, y = 0; for (var i = 0; i < ts.length; i++) { x += ts[i].clientX; y += ts[i].clientY; }
      return { x: x / ts.length, y: y / ts.length };
    }

    surface.addEventListener('touchstart', function (e) {
      e.preventDefault();
      surface.classList.add('used');
      var now = Date.now();
      st.fingers = e.touches.length;
      st.maxFingers = Math.max(st.maxFingers, st.fingers);
      var c = centroid(e.touches);
      if (st.fingers === 1) {
        st.startT = now; st.startX = st.lastX = c.x; st.startY = st.lastY = c.y; st.lastT = now; st.moved = 0; st.maxFingers = 1;
        st.dragArmed = now - st.lastTapEnd < 280;   // tap, then touch again and move = drag
      } else { st.cx = c.x; st.cy = c.y; surface.classList.add('scrolling'); }
    }, { passive: false });

    surface.addEventListener('touchmove', function (e) {
      e.preventDefault();
      var now = Date.now(), c = centroid(e.touches);
      if (e.touches.length === 1 && st.maxFingers === 1) {
        var dx = c.x - st.lastX, dy = c.y - st.lastY, dt = Math.max(1, now - st.lastT);
        st.moved += Math.abs(dx) + Math.abs(dy);
        if (st.dragArmed && !st.dragging && st.moved > 6) { st.dragging = true; api.button('left', 'down'); }
        var speed = Math.sqrt(dx * dx + dy * dy) / dt;           // px per ms
        var gain = 1.1 + Math.min(1.9, speed * 1.1);             // gentle acceleration
        pending.dx += dx * gain; pending.dy += dy * gain;
        st.lastX = c.x; st.lastY = c.y; st.lastT = now;
        schedule();
      } else if (e.touches.length >= 2) {
        var sdx = c.x - st.cx, sdy = c.y - st.cy;
        st.moved += Math.abs(sdx) + Math.abs(sdy);
        // Wheel units: 120 = one notch. Fingers moving down scroll the content down (natural direction).
        pending.sy += sdy * 4; pending.sx += sdx * 4;
        st.cx = c.x; st.cy = c.y;
        schedule();
      }
    }, { passive: false });

    function end(e) {
      e.preventDefault();
      var now = Date.now();
      if (e.touches.length === 0) {
        flush();
        var quick = now - st.startT < 280 && st.moved < 10;
        if (st.dragging) { api.button('left', 'up'); st.dragging = false; st.lastTapEnd = 0; }
        else if (quick && st.maxFingers === 1) { api.button('left', 'click'); st.lastTapEnd = now; }
        else if (quick && st.maxFingers === 2) { api.button('right', 'click'); st.lastTapEnd = 0; }
        st.maxFingers = 0; st.fingers = 0; st.dragArmed = false;
        surface.classList.remove('scrolling');
      } else {
        st.fingers = e.touches.length;
        var c = centroid(e.touches); st.cx = c.x; st.cy = c.y;
      }
    }
    surface.addEventListener('touchend', end, { passive: false });
    surface.addEventListener('touchcancel', function (e) { if (st.dragging) { api.button('left', 'up'); st.dragging = false; } end(e); }, { passive: false });

    /* Mouse fallback for desktop browsers (handy for testing). */
    var md = false;
    surface.addEventListener('mousedown', function (e) { md = true; st.lastX = e.clientX; st.lastY = e.clientY; st.startT = Date.now(); st.moved = 0; surface.classList.add('used'); });
    addEventListener('mousemove', function (e) { if (!md) return; var dx = e.clientX - st.lastX, dy = e.clientY - st.lastY; st.moved += Math.abs(dx) + Math.abs(dy); pending.dx += dx; pending.dy += dy; st.lastX = e.clientX; st.lastY = e.clientY; schedule(); });
    addEventListener('mouseup', function () { if (!md) return; md = false; flush(); if (Date.now() - st.startT < 280 && st.moved < 10) api.button('left', 'click'); });

    /* ---- on-screen keyboard: summon it on demand and forward what is typed ---- */
    function resetKb() { kb.value = KB_SENTINEL; try { kb.setSelectionRange(KB_SENTINEL.length, KB_SENTINEL.length); } catch (e) {} }
    function sendText(s) {
      if (!s) return;
      var a = activeMods();
      if (a.length && s.length === 1) { api.key(s.toLowerCase(), a); clearMods(); }   // Ctrl + c etc.
      else api.text(s);
    }
    kbBtn.addEventListener('click', function () {
      if (document.activeElement === kb) { kb.blur(); return; }
      resetKb();
      kb.focus({ preventScroll: true });
    });
    kb.addEventListener('focus', function () { kbBtn.classList.add('on'); resetKb(); });
    kb.addEventListener('blur', function () { kbBtn.classList.remove('on'); });
    kb.addEventListener('beforeinput', function (e) {
      switch (e.inputType) {
        case 'insertText': if (e.data) sendText(e.data); break;
        case 'insertLineBreak': case 'insertParagraph': withMods(function (m) { api.key('enter', m); }); break;
        case 'deleteContentBackward': withMods(function (m) { api.key('backspace', m); }); break;
        case 'deleteContentForward': api.key('delete', []); break;
        default: break;   // composition text is sent when the word is committed
      }
    });
    kb.addEventListener('compositionend', function (e) { if (e.data) sendText(e.data); });
    kb.addEventListener('input', function (e) { if (!e.isComposing) resetKb(); });
    kb.addEventListener('keydown', function (e) {
      // Hardware / some soft keyboards report named keys instead of beforeinput.
      var map = { Enter: 'enter', Backspace: 'backspace', Tab: 'tab', Escape: 'esc', ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down', Delete: 'delete' };
      if (e.key === 'Enter' || e.key === 'Backspace') return;   // handled by beforeinput
      if (map[e.key]) { e.preventDefault(); api.key(map[e.key], activeMods()); clearMods(); }
    });

    /* Keep the pad above the on-screen keyboard. */
    if (window.visualViewport) {
      var fit = function () { root.style.height = Math.max(240, window.visualViewport.height - root.getBoundingClientRect().top - 8) + 'px'; };
      window.visualViewport.addEventListener('resize', fit);
      window.visualViewport.addEventListener('scroll', fit);
    }

    return { keyboard: function () { kbBtn.click(); }, destroy: function () { root.textContent = ''; } };
  };
})();
