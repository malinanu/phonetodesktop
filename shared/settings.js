/* Settings shared by every page. In the Android app they live in the app (one place for every
   WebView, even though the pages have different origins); in a normal browser in localStorage.
   Load this in <head> so the theme is applied before the first paint. */
(function () {
  'use strict';
  var DEFAULTS = { theme: 'system', skip: 10, volStep: 2, padSpeed: 1, scroll: 'natural', tapToClick: true, vibrate: true, keepAwake: false };
  var app = window.AndroidBridge && window.AndroidBridge.getSettings ? window.AndroidBridge : null;
  var listeners = [];

  function load() {
    var raw = null;
    try { raw = app ? app.getSettings() : localStorage.getItem('pr.settings'); } catch (e) {}
    var stored = {};
    try { stored = JSON.parse(raw || '{}') || {}; } catch (e) {}
    var out = {};
    Object.keys(DEFAULTS).forEach(function (k) { out[k] = stored[k] !== undefined && typeof stored[k] === typeof DEFAULTS[k] ? stored[k] : DEFAULTS[k]; });
    return out;
  }

  var cur = load();

  function applyTheme() {
    var root = document.documentElement;
    if (cur.theme === 'dark' || cur.theme === 'light') root.setAttribute('data-theme', cur.theme);
    else root.removeAttribute('data-theme');
  }

  function set(patch) {
    Object.keys(patch).forEach(function (k) { if (k in DEFAULTS) cur[k] = patch[k]; });
    var json = JSON.stringify(cur);
    try { if (app) app.saveSettings(json); else localStorage.setItem('pr.settings', json); } catch (e) {}
    applyTheme();
    listeners.forEach(function (f) { f(cur); });
  }

  applyTheme();
  window.PRSettings = {
    get: function () { return cur; },
    set: set,
    defaults: DEFAULTS,
    inApp: !!app,
    onChange: function (f) { listeners.push(f); },
    /** Vibrate unless the user turned it off. */
    haptic: function (ms) { if (cur.vibrate && navigator.vibrate) navigator.vibrate(ms); },
  };
})();
