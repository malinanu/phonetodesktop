// "My devices": sign in with an emailed code and manage the devices on an optional Lanyard account.
// Talks to the account service (account/). Tokens: the short-lived access token stays in memory; the refresh
// token is kept in sessionStorage (gone when the tab closes). Everything shown is set with textContent.
(function () {
  "use strict";
  var API = "https://lanyardfileshare.3logiq.com/account";
  var $ = function (id) { return document.getElementById(id); };
  var access = null;
  var email = "";

  function store(key, value) {
    try { value == null ? sessionStorage.removeItem(key) : sessionStorage.setItem(key, value); } catch (e) { /* private mode */ }
  }
  function load(key) {
    try { return sessionStorage.getItem(key); } catch (e) { return null; }
  }

  function say(text, bad) {
    var m = $("msg");
    m.hidden = !text;
    m.textContent = text || "";
    m.className = "acct-msg" + (bad ? " bad" : "");
  }

  async function call(method, path, body, token) {
    var headers = {};
    if (body) headers["Content-Type"] = "application/json";
    if (token) headers["Authorization"] = "Bearer " + token;
    var res;
    try {
      res = await fetch(API + path, { method: method, headers: headers, body: body ? JSON.stringify(body) : undefined });
    } catch (e) {
      throw { status: 0, message: "Could not reach the account service. Check your connection." };
    }
    if (res.status === 204) return null;
    var data = null;
    try { data = await res.json(); } catch (e) { /* no body */ }
    if (!res.ok) {
      var detail = data && typeof data.detail === "string" ? data.detail : "The account service answered " + res.status + ".";
      if (res.status === 422) detail = "Please check what you typed.";
      throw { status: res.status, message: detail };
    }
    return data;
  }

  function takeTokens(t) {
    access = t.access_token;
    email = t.user.email;
    store("lanyard.refresh", t.refresh_token);
  }

  // Calls that need a signed-in user: if the access token ran out, use the refresh token once and try again.
  async function authed(method, path, body) {
    if (!access) await refresh();
    try {
      return await call(method, path, body, access);
    } catch (e) {
      if (e.status !== 401) throw e;
      await refresh();
      return call(method, path, body, access);
    }
  }

  async function refresh() {
    var r = load("lanyard.refresh");
    if (!r) throw { status: 401, message: "Sign in again." };
    try {
      takeTokens(await call("POST", "/v1/auth/refresh", { refresh_token: r }));
    } catch (e) {
      store("lanyard.refresh", null);
      access = null;
      if (e.status === 401) { show(false); throw { status: 401, message: "Your sign-in ended. Sign in again." }; }
      throw e;
    }
  }

  function show(signedIn) {
    $("signin").hidden = signedIn;
    $("devices").hidden = !signedIn;
  }

  var PLATFORMS = { android: "Android", ios: "iPhone", windows: "Windows", macos: "Mac", linux: "Linux" };

  function when(unix) {
    return new Date(unix * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
  }

  function row(d) {
    var li = document.createElement("li");
    li.className = "acct-dev" + (d.revoked ? " gone" : "");
    var info = document.createElement("div");
    var name = document.createElement("b");
    name.textContent = d.name;
    var meta = document.createElement("span");
    meta.className = "dim";
    meta.textContent = (d.kind === "pc" ? "Computer" : "Phone") + " · " + (PLATFORMS[d.platform] || d.platform) + " · added " + when(d.created_at) + (d.revoked ? " · removed" : "");
    info.append(name, document.createElement("br"), meta);
    li.append(info);
    if (!d.revoked) {
      var actions = document.createElement("div");
      actions.className = "acct-actions";
      var rename = document.createElement("button");
      rename.type = "button"; rename.className = "btn line small"; rename.textContent = "Rename";
      rename.addEventListener("click", function () { renameDevice(d); });
      var remove = document.createElement("button");
      remove.type = "button"; remove.className = "btn line small"; remove.textContent = "Remove";
      remove.addEventListener("click", function () { removeDevice(d); });
      actions.append(rename, remove);
      li.append(actions);
    }
    return li;
  }

  async function reload() {
    var data = await authed("GET", "/v1/devices");
    $("who").textContent = email;
    var list = $("list");
    list.replaceChildren.apply(list, data.devices.map(row));
    $("empty").hidden = data.devices.length > 0;
    show(true);
  }

  async function guard(fn) {
    say("");
    try { await fn(); } catch (e) { say(e.message || "Something went wrong.", true); }
  }

  function renameDevice(d) {
    var next = window.prompt("New name for this device", d.name);
    if (next == null || !next.trim()) return;
    guard(async function () { await authed("PATCH", "/v1/devices/" + encodeURIComponent(d.id), { name: next.trim() }); await reload(); });
  }

  function removeDevice(d) {
    var warn = d.kind === "pc" ? "This computer will stop accepting the account's phones within about 15 minutes." : "This phone will stop being accepted by your computers within about 15 minutes.";
    if (!window.confirm("Remove " + d.name + "? " + warn)) return;
    guard(async function () { await authed("DELETE", "/v1/devices/" + encodeURIComponent(d.id)); await reload(); say(d.name + " was removed. Computers learn about it the next time they check, about every 15 minutes."); });
  }

  $("emailForm").addEventListener("submit", function (ev) {
    ev.preventDefault();
    guard(async function () {
      email = $("email").value.trim();
      await call("POST", "/v1/auth/email", { email: email });
      $("sentTo").textContent = "We sent a code to " + email + ".";
      $("emailForm").hidden = true;
      $("codeForm").hidden = false;
      $("code").focus();
    });
  });

  $("backBtn").addEventListener("click", function () {
    $("codeForm").hidden = true;
    $("emailForm").hidden = false;
    say("");
  });

  $("codeForm").addEventListener("submit", function (ev) {
    ev.preventDefault();
    guard(async function () {
      takeTokens(await call("POST", "/v1/auth/verify", { email: email, code: $("code").value.trim() }));
      $("code").value = "";
      await reload();
    });
  });

  $("linkBtn").addEventListener("click", function () {
    guard(async function () {
      var out = await authed("POST", "/v1/links");
      var p = $("linkOut");
      p.hidden = false;
      p.textContent = out.code;
      say("Type this code into Lanyard on the computer within " + Math.round(out.expires_in / 60) + " minutes.");
    });
  });

  $("outBtn").addEventListener("click", function () {
    guard(async function () {
      var r = load("lanyard.refresh");
      store("lanyard.refresh", null);
      access = null;
      if (r) { try { await call("POST", "/v1/auth/logout", { refresh_token: r }); } catch (e) { /* the token is gone from this tab anyway */ } }
      $("linkOut").hidden = true;
      $("emailForm").hidden = false;
      $("codeForm").hidden = true;
      show(false);
    });
  });

  $("delBtn").addEventListener("click", function () {
    if (!window.confirm("Delete your account? This removes your email and every device from it. It cannot be undone.")) return;
    guard(async function () {
      await authed("DELETE", "/v1/me");
      store("lanyard.refresh", null);
      access = null;
      show(false);
      say("Your account was deleted.");
    });
  });

  if (load("lanyard.refresh")) {
    guard(reload).then(function () { if ($("devices").hidden) show(false); });
  } else {
    show(false);
  }
})();
