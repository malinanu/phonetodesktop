// The Downloads page: lists every file of the latest release (latest.json, written by the release workflow) with its
// size and SHA-256 and a button that links straight to the file on this server. Without latest.json (no release yet)
// it still shows every platform and points at the GitHub releases page.
(function () {
  "use strict";
  var RELEASES = "https://github.com/malinanu/phonetodesktop/releases/latest";
  var NS = "http://www.w3.org/2000/svg";

  function meta(name) {
    var m = document.querySelector('meta[name="' + name + '"]');
    var v = m ? m.getAttribute("content") : "";
    return /^https:\/\//.test(v) ? v : "";
  }
  var APP_STORE = meta("lanyard:app-store"), PLAY_STORE = meta("lanyard:play-store");

  function detect() {
    var ua = navigator.userAgent || "";
    var p = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
    if (/android/i.test(ua)) return "android";
    if (/iPhone|iPad|iPod/i.test(ua) || (/Mac/i.test(p) && navigator.maxTouchPoints > 1)) return "ios";
    if (/Win/i.test(p) || /Windows/i.test(ua)) return "windows";
    if (/Mac/i.test(p) || /Macintosh/i.test(ua)) return "macos";
    if (/Linux|X11|CrOS/i.test(p + ua)) return "linux";
    return "other";
  }
  var mine = detect();
  var arm = /aarch64|arm64|armv8/i.test((navigator.userAgent || "") + ((navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "")) ? "aarch64" : "x86_64";

  function size(n) { return n >= 1048576 ? (n / 1048576).toFixed(1) + " MB" : Math.max(1, Math.round(n / 1024)) + " KB"; }
  function el(tag, cls, text) { var e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; }
  function icon(id) {
    var s = document.createElementNS(NS, "svg"); s.setAttribute("aria-hidden", "true");
    var u = document.createElementNS(NS, "use"); u.setAttribute("href", "#" + id); s.appendChild(u); return s;
  }
  // Links must be https (plain http only for local testing) or a plain relative path; nothing else becomes a link.
  function safe(url) { return /^https:\/\//.test(url) || /^http:\/\/(127\.0\.0\.1|localhost)[:\/]/.test(url) || /^[A-Za-z0-9._\/-]+$/.test(url) ? url : ""; }

  // One row per file: icon, name and details, the checksum, and the Download button.
  function row(o) {
    var hl = o.highlight !== undefined ? o.highlight && o.os === mine : o.os === mine;
    var d = el("div", "dl-row" + (hl ? " mine" : ""));
    if (hl) d.setAttribute("data-mine", "");
    if (o.id) d.id = o.id;
    var ic = el("div", "os"); ic.appendChild(icon(o.icon));
    var w = el("div"); w.appendChild(el("b", null, o.title)); w.appendChild(el("span", "meta", o.sub));
    if (o.sha) {
      var h = el("span", "meta"); h.appendChild(document.createTextNode("SHA-256 ")); h.appendChild(el("code", "sha", o.sha)); w.appendChild(h);
    }
    if (o.mirror && safe(o.mirror)) {
      var m = el("span", "meta"); var ma = el("a", null, "Mirror on GitHub"); ma.href = o.mirror; m.appendChild(ma); w.appendChild(m);
    }
    d.appendChild(ic); d.appendChild(w);
    if (o.url && safe(o.url)) {
      var a = el("a", "btn" + (hl ? " primary" : ""), o.label || "Download"); a.href = o.url;
      if (!o.store) a.setAttribute("download", "");
      a.setAttribute("aria-label", (o.label || "Download") + " " + o.title);
      d.appendChild(a);
    } else {
      d.appendChild(el("span", "soon", o.label || "Coming soon"));
    }
    return d;
  }

  var ARCH = { x86_64: "64-bit Intel/AMD", aarch64: "ARM64", universal: "Apple silicon and Intel" };
  var KIND = { installer: "Installer", deb: "Debian and Ubuntu (.deb)", tarball: "Other distributions (.tar.gz)", apk: "Android app (.apk)" };

  function fileRow(a, os, iconId, title, first) {
    var sub = [];
    if (a.platform === "linux") {
      title = "Linux · " + (a.kind === "deb" ? "Debian and Ubuntu (.deb)" : "Other distributions (.tar.gz)");
      sub.push(ARCH[a.arch] || a.arch);
    } else {
      sub.push(KIND[a.kind] || a.kind);
    }
    sub.push(size(a.size));
    // Only the file that suits this computer is highlighted: for Linux, the .deb of the right architecture.
    var suits = first && (os !== "linux" || a.arch === arm);
    return row({ os: os, icon: iconId, title: title, sub: sub.join(" · "), sha: a.sha256, url: a.url, mirror: a.mirror, id: first ? os : "", label: "Download", highlight: suits });
  }

  function render(m) {
    var computers = document.getElementById("dl-computers"), phones = document.getElementById("dl-phones");
    computers.textContent = ""; phones.textContent = "";
    var assets = (m && m.assets) || [];
    var by = function (platform) { return assets.filter(function (a) { return a.platform === platform; }); };
    var added = { windows: false, macos: false, linux: false, android: false };

    [["windows", "i-desktop", "Windows 10 and 11"], ["macos", "i-laptop", "macOS 11 and newer"], ["linux", "i-terminal", "Linux"]].forEach(function (p) {
      var list = by(p[0]);
      if (p[0] === "linux") {
        // The .deb before the tarball, and this computer's architecture first.
        list.sort(function (x, y) {
          var rank = function (a) { return (a.arch === arm ? 0 : 2) + (a.kind === "deb" ? 0 : 1); };
          return rank(x) - rank(y);
        });
      }
      list.forEach(function (a, i) { computers.appendChild(fileRow(a, p[0], p[1], p[2], i === 0)); added[p[0]] = true; });
      if (!list.length) computers.appendChild(row({ os: p[0], icon: p[1], title: p[2], sub: "Download from the releases page", url: RELEASES, label: "See downloads", id: p[0] }));
    });

    if (PLAY_STORE) phones.appendChild(row({ os: "android", icon: "i-phone", title: "Android, Google Play", sub: "Android 9 and newer", url: PLAY_STORE, store: true, label: "Get it on Google Play", id: "android" }));
    by("android").forEach(function (a, i) { phones.appendChild(fileRow(a, "android", "i-phone", PLAY_STORE ? "Android, direct download" : "Android", i === 0 && !PLAY_STORE)); added.android = true; });
    if (!PLAY_STORE && !added.android) phones.appendChild(row({ os: "android", icon: "i-phone", title: "Android", sub: "Android 9 and newer (.apk)", url: RELEASES, label: "See downloads", id: "android" }));
    phones.appendChild(row({ os: "ios", icon: "i-phone", title: "iPhone and iPad", sub: APP_STORE ? "App Store" : "The app is being prepared. For now, open the code on your computer with the iPhone camera and use the remote in Safari.", url: APP_STORE, store: true, label: APP_STORE ? "Download on the App Store" : "Coming soon", id: "ios" }));

    var v = document.getElementById("dl-version"), foot = document.getElementById("dl-foot");
    if (m && m.version) {
      v.textContent = "Latest version " + m.version + (m.released ? ", released " + new Date(m.released).toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" }) : "") + ". Free and open source.";
      var bits = [];
      if (m.checksums && safe(m.checksums)) { var c = '<a href="' + m.checksums.replace(/"/g, "") + '">All checksums (SHA256SUMS)</a>'; bits.push(c); }
      if (m.github_release_page && safe(m.github_release_page)) bits.push('<a href="' + m.github_release_page.replace(/"/g, "") + '">This version on GitHub</a>');
      foot.innerHTML = bits.join(" · ");
    } else {
      v.textContent = "The first release is not published yet. The links below lead to the releases page.";
    }
    // Land on the section the visitor asked for (index.html links here with #windows, #android, ...).
    var t = location.hash && document.getElementById(location.hash.slice(1));
    if (t) { t.scrollIntoView(); t.setAttribute("tabindex", "-1"); t.focus({ preventScroll: true }); }
  }

  // Open the install help that matches this computer.
  var help = document.getElementById("help-" + mine);
  if (help) help.open = true;

  fetch("latest.json", { cache: "no-cache" })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (m) { render(m && m.assets ? m : null); })
    .catch(function () { render(null); });
})();
