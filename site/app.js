(function () {
  "use strict";
  var DOWNLOADS = "downloads.html";
  var NS = "http://www.w3.org/2000/svg";

  function meta(name) {
    var m = document.querySelector('meta[name="' + name + '"]');
    var v = m ? m.getAttribute("content") : "";
    return /^https:\/\//.test(v) ? v : "";
  }
  var APP_STORE = meta("lanyard:app-store"), PLAY_STORE = meta("lanyard:play-store");

  // ---- which computer / phone is this? ----
  function detect() {
    var ua = navigator.userAgent || "";
    var uaPlatform = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
    var os = "other";
    if (/android/i.test(ua)) os = "android";
    else if (/iPhone|iPad|iPod/i.test(ua) || (/Mac/i.test(uaPlatform) && navigator.maxTouchPoints > 1)) os = "ios";
    else if (/Win/i.test(uaPlatform) || /Windows/i.test(ua)) os = "windows";
    else if (/Mac/i.test(uaPlatform) || /Macintosh/i.test(ua)) os = "macos";
    else if (/Linux|X11|CrOS/i.test(uaPlatform + ua)) os = "linux";
    var arm = /aarch64|arm64|armv8/i.test(ua + uaPlatform);
    return { os: os, arch: arm ? "aarch64" : "x86_64" };
  }
  var me = detect();

  function size(n) { return n >= 1048576 ? (n / 1048576).toFixed(1) + " MB" : Math.max(1, Math.round(n / 1024)) + " KB"; }
  function el(tag, cls, text) { var e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; }
  function icon(id) {
    var s = document.createElementNS(NS, "svg"); s.setAttribute("aria-hidden", "true");
    var u = document.createElementNS(NS, "use"); u.setAttribute("href", "#" + id); s.appendChild(u); return s;
  }

  // One row per platform: icon, name, what it is, and the button.
  function row(os, iconId, title, sub, link, label, mine) {
    var d = el("div", "dl-row"); if (mine) d.setAttribute("data-mine", "");
    var ic = el("div", "os"); ic.appendChild(icon(iconId));
    var w = el("div"); w.appendChild(el("b", null, title)); w.appendChild(el("span", "meta", sub));
    d.appendChild(ic); d.appendChild(w);
    if (link) {
      var a = el("a", "btn" + (mine ? " primary" : ""), label || "Download"); a.href = link;
      if (!/^https?:\/\/(play\.google|apps\.apple)/.test(link)) a.setAttribute("download", "");
      a.setAttribute("aria-label", (label || "Download") + " " + title);
      d.appendChild(a);
    } else {
      d.appendChild(el("span", "soon", label || "Coming soon"));
    }
    return d;
  }

  function find(assets, platform, kind, arch) {
    return assets.filter(function (a) {
      return a.platform === platform && a.kind === kind && (!arch || a.arch === arch || a.arch === "universal");
    });
  }

  function render(m) {
    var box = document.getElementById("dl-list");
    box.textContent = "";
    var assets = (m && m.assets) || [];
    var meta = function (a, extra) { return (extra ? extra + " · " : "") + size(a.size) + " · SHA-256 " + a.sha256.slice(0, 12) + "…"; };
    var best = null;

    // Windows
    var win = find(assets, "windows", "installer")[0];
    if (win) { if (me.os === "windows") best = win; box.appendChild(row("windows", "i-desktop", "Windows", "Windows 10 and 11 · " + meta(win), win.url, "Download", me.os === "windows")); }
    // macOS
    var mac = find(assets, "macos", "installer")[0];
    if (mac) { if (me.os === "macos") best = mac; box.appendChild(row("macos", "i-laptop", "macOS", "Apple silicon and Intel · " + meta(mac), mac.url, "Download", me.os === "macos")); }
    // Linux
    var debs = find(assets, "linux", "deb", me.arch), tars = find(assets, "linux", "tarball", me.arch);
    var lin = debs[0] || tars[0] || find(assets, "linux", "deb")[0] || find(assets, "linux", "tarball")[0];
    if (lin) {
      if (me.os === "linux") best = lin;
      var kind = lin.kind === "deb" ? "Debian and Ubuntu (.deb)" : "Other distributions (.tar.gz)";
      box.appendChild(row("linux", "i-terminal", "Linux", kind + " · " + (lin.arch === "aarch64" ? "ARM64" : "x86-64") + " · " + meta(lin), lin.url, "Download", me.os === "linux"));
      var other = lin.kind === "deb" ? tars[0] : debs[0];
      if (other) box.appendChild(row("linux", "i-terminal", "Linux, other format", (other.kind === "deb" ? "Debian and Ubuntu (.deb)" : "Other distributions (.tar.gz)") + " · " + meta(other), other.url, "Download", false));
    }
    if (!box.children.length) {
      // No release manifest yet (or it could not be loaded): every platform still gets a clear way in.
      [["windows", "i-desktop", "Windows", "Windows 10 and 11"], ["macos", "i-laptop", "macOS", "Apple silicon and Intel"], ["linux", "i-terminal", "Linux", "Debian, Ubuntu and others"]].forEach(function (r) {
        box.appendChild(row(r[0], r[1], r[2], r[3], DOWNLOADS + "#" + r[0], "See downloads", me.os === r[0]));
      });
    }
    // Phones
    var apk = find(assets, "android", "apk")[0];
    if (PLAY_STORE) box.appendChild(row("android", "i-phone", "Android", "Google Play", PLAY_STORE, "Get it on Google Play", me.os === "android"));
    if (!apk && !PLAY_STORE) box.appendChild(row("android", "i-phone", "Android", "Android 9 and newer", DOWNLOADS + "#android", "See downloads", me.os === "android"));
    if (apk) box.appendChild(row("android", "i-phone", PLAY_STORE ? "Android, direct download" : "Android", "Android 9 and newer (.apk) · " + meta(apk), apk.url, "Download", me.os === "android" && !PLAY_STORE));
    box.appendChild(row("ios", "i-phone", "iPhone and iPad", APP_STORE ? "App Store" : "The app is being prepared. For now, open the code on your computer with the iPhone camera and use the remote in Safari.", APP_STORE, APP_STORE ? "Download on the App Store" : "Coming soon", me.os === "ios" && !!APP_STORE));

    // Hero and closing buttons
    var main = document.getElementById("dl-main"), close = document.getElementById("dl-close"), sub = document.getElementById("dl-sub");
    if (best) {
      var names = { windows: "Windows", macos: "macOS", linux: "Linux" };
      var label = "Download for " + names[best.platform];
      main.textContent = label; main.href = best.url; main.setAttribute("download", "");
      close.textContent = label; close.href = best.url; close.setAttribute("download", "");
      sub.textContent = "Version " + m.version + " · " + size(best.size) + " · free and open source";
    } else if (me.os === "android" || me.os === "ios") {
      main.textContent = "Get the phone app"; sub.textContent = "Install Lanyard on your computer too. The list below has every download.";
    }
    if (m && m.version) document.getElementById("dl-foot").textContent = "Latest version " + m.version + ".";
  }

  fetch("latest.json", { cache: "no-cache" })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (m) { render(m && m.assets ? m : null); })
    .catch(function () { render(null); });

  // ---- questions: one card open at a time ----
  var faq = document.querySelector("[data-one-open]");
  if (faq) faq.addEventListener("toggle", function (e) {
    if (!e.target.open) return;
    faq.querySelectorAll("details").forEach(function (d) { if (d !== e.target) d.open = false; });
  }, true);

  // ---- entrances: parts rise once as they scroll into view ----
  var items = document.querySelectorAll(".rise");
  if ("IntersectionObserver" in window && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) { if (en.isIntersecting) { en.target.classList.add("in"); io.unobserve(en.target); } });
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.05 });
    items.forEach(function (i) { io.observe(i); });
  } else {
    items.forEach(function (i) { i.classList.add("in"); });
  }
})();
