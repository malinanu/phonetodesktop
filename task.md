# task.md — merge FileSync into Phone Remote

Decisions: **unified product** (one repo, one brand, in-app entry points) · FileSync server on **your own VPS**.
Source: https://github.com/polius/FileSync (MIT). Tick each box when the task is finished and verified.

## Phase 0 — Setup
- [x] T0.1 Create this `task.md`
- [x] T0.2 Work on the existing branch; update PR #1 description (one branch, one PR)
- [ ] T0.3 Confirm ownership / licence of FileSync with the owner (MIT — attribution kept either way)

## Phase 1 — Import and CI
- [x] T1.1 Import FileSync into `filesync/` as a plain copy of upstream @ 5283525 (LICENSE kept; no subtree merge commits)
- [x] T1.2 Move FileSync workflows to root `.github/`, scoped to `filesync/**`
- [x] T1.3 Run `ruff`, `pytest`, `npm run lint`, `npm test` locally — all green
- [x] T1.4 Root README: add `filesync/` and a "Send files" section
- [x] T1.5 Handle the CC BY-NC xkcd comic (remove or keep attribution)

## Phase 2 — One look and feel
- [x] T2.1 `phoneremote-theme.css` maps FileSync colours onto `shared/base.css` tokens
- [x] T2.2 Ship the shared font in FileSync (CSP `font-src 'self'`)
- [x] T2.3 User-facing strings: "Phone Remote · Send files", back link, FileSync credit
- [x] T2.4 Contrast ≥ 4.5:1 in both themes

## Phase 3 — Server, deploy, release (VPS)
- [x] T3.1 `deploy/docker-compose.yml` + `deploy/Caddyfile` (HTTPS, coturn, secret init)
- [x] T3.2 Release workflow builds and pushes the image to GHCR on `v*` tags
- [x] T3.3 `docs/DEPLOYING-SERVER.md` runbook
- [x] T3.4 Hardening notes (real client IP, TURN limits, exposed paths)
- [ ] T3.5 Local compose smoke test (needs Docker + two browsers) — partly done: CI's `docker` build and `e2e-smoke` (the real FileSync container stack: nginx + FastAPI + coturn, 100-MB-class transfer in Chromium) are green; app also tested here without Docker. Not yet run: `deploy/docker-compose.yml` itself (Caddy + GHCR image) on a real server

## Phase 4 — Android "Files" tab
Changed approach: FileSync streams downloads through a service worker / File System Access API, which an Android WebView cannot hand to the system downloader (and I won't expose a JS bridge to a remote site). So the tab is a native panel that opens Send files in the phone's browser, where large receives work.
- [x] T4.1 Third "Files" tab (native panel, no remote WebView, no JS bridge)
- [x] T4.2 Server URL via `-PfilesUrl` → `BuildConfig.FILES_URL`, overridable in Settings; https-only validation (`FilesUrl.kt`) + unit tests
- [x] T4.3 ~~File chooser / download handling in a WebView~~ not needed: the browser does both
- [x] T4.4 Opens in the browser (that is the "large receive" path)
- [x] T4.5 Permissions re-checked: none added (INTERNET already declared)
- [ ] T4.6 Stretch: Android share-target (not done)
- [x] T4.7a CI compiled the app and ran `testDebugUnitTest` (incl. `FilesUrlTest`): green
- [ ] T4.7b Try the Files tab on a real phone

## Phase 5 — Windows agent entry points
- [x] T5.1 Config key `files_url`
- [x] T5.2 Dashboard card + tray item "Send files"
- [ ] T5.3 Stretch (not done): "Send to this PC" QR
- [x] T5.4 Rust tests

## Phase 6 — Website, docs, privacy, store
- [x] T6.1 Landing page: "Send files" section
- [x] T6.2 Rewrite privacy policy (a server now sees IPs/peer ids transiently)
- [x] T6.3 Update Play Data safety answers in `docs/RELEASING.md`
- [x] T6.4 Update RELEASING.md + guide page
- [x] T6.5 Release checklist: server up and healthy before tagging

## Phase 7 — Verify and ship
- [x] T7.1 FileSync browser e2e: smoke cell (Chromium / service-worker sink / 20 MiB, SHA-256 verified) passes against the app run without Docker (stand-in for nginx). Not run: full matrix, interruption harness and TURN cells (need Docker + coturn)
- [ ] T7.2 Real-device matrix incl. cross-network (TURN relay)
- [x] T7.3 `/security-review` of the new surface (no high-confidence findings)
- [ ] T7.4 CI green, PR ready, tag `v1.1.0` only after T3.5 and T7.2

## Portainer
- [x] Portainer-ready stack `deploy/docker-compose.portainer.yml` (inline Caddy config, optional `TURN_EXTERNAL_IP`) + runbook section. Rendered and checked with `docker compose config`; not run on a real Portainer/Docker host

## Phase 8 — Cross-platform product (plan: optional cloud account, Flutter mobile)
Order: A+B first (ship as a release), then C, then E and D in parallel, then F, then G.

### A. Cross-platform desktop agent
- [x] A1 Linux `Backend`: MPRIS (zbus), volume via wpctl/pactl, input via enigo (X11/XTest). Verified live here: MPRIS roundtrip against a fake player on a private D-Bus, and X11 mouse/scroll/text/Ctrl+key/media-key events seen by a probe window under Xvfb. Not done: uinput for Wayland-only desktops (documented limit)
- [ ] A2 macOS `Backend` (enigo media keys + input, volume via osascript; no now-playing): implemented; compiles, unit tests pass on a macOS CI runner, and the dmg builds there; the actual input/media-key behaviour is NOT verified on a real Mac (needs the G1 manual check)
- [ ] A3 Tray / autostart / log folder / open-url cross-platform: DONE for open-url (open / xdg-open), start-at-login (macOS LaunchAgent with KeepAlive, Linux XDG autostart; `phone-remote autostart on|off|status`; dashboard toggle), log folder (already per-OS), and the `phone-remote open` launcher (starts the agent detached, shows the dashboard); verified live on Linux. NOT done: a tray icon on macOS/Linux (the dashboard in the browser is the UI there for now); macOS parts compile-checked only
- [ ] A4 Packaging + CI matrix. DONE and verified here: Linux .deb (built, inspected, extracted and run) + generic tarball with `install.sh` (installed/removed in a throwaway home); CI jobs `agent-linux` (tests, clippy, live D-Bus + X11 tests, package smoke) and `agent-macos`; release jobs for Linux amd64/arm64 and macOS universal; `actionlint` clean. Changed: no AppImage (the .deb + tarball cover it). CI (head 52cab92) is green for both new jobs: `agent-macos` built the universal binary and the .dmg, then `hdiutil verify`, `codesign --verify` and `plutil -lint` passed on a real macOS runner; `agent-linux` ran the live D-Bus + X11 tests and the .deb/tarball smoke test on Ubuntu 22.04. Still unexercised: the release jobs themselves (only run on a `v*` tag, incl. the arm64 runner) and signing/notarization (needs the Apple secrets)
- [x] A5 mDNS + firewall notes per OS: website first-run notes (Windows private network, macOS Local Network + Accessibility, Linux X11/XWayland); `Info.plist` has NSLocalNetworkUsageDescription + NSBonjourServices. Not verified on a real Mac/Linux LAN

### B. Website downloads
- [x] B1 OS detection and per-platform downloads + checksums + store badges (hidden until URLs exist): rendered in Chromium with a generated latest.json under Windows, macOS, Linux ARM64 and Android user agents; no JS errors, no overflow at phone width
- [x] B2 Release workflow publishes `latest.json` + `SHA256SUMS` (`installer/make_latest.py`, 5 tests); Pages workflow fetches it at deploy time and rebuilds on each release; store/files links filled from repo variables with strict validation
- [x] B3 Privacy policy, README and first-run notes per platform (SmartScreen, Gatekeeper, Linux)

### C. Local device identity (auth v2)
- [x] C1 Protocol v2 in the agent: Ed25519 public key at pairing (no token handed out), per-connection challenge/response bound to PC id + device id + nonce, v1 behind `allow_v1`; documented in docs/PROTOCOL.md. Verified: 10 unit tests (replay, wrong PC/device/key, malformed, key takeover) and 5 real-WebSocket end-to-end tests
- [x] C2 Trust list stores platform / pubkey; dashboard Phones page shows type, login method and last seen; new switch 'Allow older phone apps'
- [x] C3 ~~Optional PC PIN~~ dropped: the owner already has to approve every new device on the PC, and a changed key is re-approved
- [x] C4 Tests incl. migration: configs from the previous version (device with token, no pubkey) load and round-trip

- [x] C5 (added after your choice of pinned HTTPS) TLS with a per-PC self-signed ECDSA P-256 certificate; HTTPS + HTTP on one port (first-byte sniffing, handshake in its own task so a silent client cannot block others); fingerprint in the QR; plain HTTP refused from the network when older phones are off. Verified with the real binary: QR fingerprint == openssl's SHA-256 of the served public key; `curl --pinnedpubkey` accepts the right pin and rejects a wrong one; TLS 1.3 + ALPN http/1.1; silent connection does not delay others; plain-from-network refused when v1 off; plus a test with a real pinning rustls client doing the v2 login over wss and a wrong pin refused. CI (head f696949) is green on the macOS and Windows runners too, so the `ring` C build and the TLS code pass there

### D. Account service (optional cloud)
- [x] D1 `account/` FastAPI + SQLAlchemy (SQLite for tests, Postgres in deploy): users, devices, signed certificates (Ed25519, key ids for rotation), signed revocation list, link codes; 52 tests (pytest) incl. authorization between accounts, tampered/expired certs, rate limits; also smoke-tested over real HTTP with uvicorn
- [x] D2 Sign-in: emailed one-time code (changed from a link: works in the app without deep links; 5 tries, 15 min, hashed), Google and Apple ID-token verification (tested with a locally generated RSA key standing in for the provider's JWKS; NOT tested against the real providers, which needs your client IDs), rotating refresh tokens with reuse detection
- [ ] D3 Client enrol, certificate refresh, offline verification, revocation polling. Done: the PC verifies certificates and signed revocation lists offline (`agent/src/account.rs`, checked against certificates made by the real Python service) and accepts account phones at login (`auth_sig` + `cert`), 7 new tests. Open: joining an account from the PC (link code), revocation polling, dashboard account section, Dart-side enrol and certificate renewal
- [ ] D4 Web "My devices" + account deletion
- [x] D5 Deploy: Dockerfile, optional `--profile accounts` in both compose files (Postgres + service; reachable at /account/ through the existing Caddy), CI workflow + GHCR image on tags, docs/ACCOUNTS.md. Compose rendering verified; NOT run on a real host. Backups and a real SMTP provider are yours to set up
- [ ] D6 Security review + threat-model note

### E. Flutter mobile app (iOS + Android)
- [x] E1 `mobile/` Android + iOS projects: application id `app.phoneremote`, minSdk 28, release signing from the same env keystore as the native app, permissions (Wi-Fi/multicast/camera), iOS camera + local-network + Bonjour keys; CI builds the release APK and an unsigned iOS app (first runs pending)
- [x] E2 Wi-Fi mode done natively (no WebView): pinned-TLS protocol client verified against the real Rust agent, mDNS, QR pairing, saved PCs, native remote + touchpad screens; 46 tests
- [x] E3 Android-only Bluetooth mode: `HidRemote.kt`/`HidKeys.kt` reused through a platform channel (`HidBridge.kt`), Dart controller + screen + tab (hidden on iOS), usable without pairing a PC; Dart side tested with a fake platform. The Kotlin compiles only in CI and the Bluetooth link needs a real phone + PC to verify
- [ ] E4 Files tab, Settings and themes done (widget tests); contrast audit and screen-reader pass still open
- [ ] E5 Device identity + optional sign-in
- [ ] E6 Parity checklist, then retire `android/`; CI builds AAB and IPA

### F. Stores
- [ ] F1 Apple: developer account, App Store Connect, TestFlight, local-network strings, privacy labels
- [ ] F2 Google Play: Data safety + privacy policy updated for accounts
- [ ] F3 Release automation and listings

### G. Verify and ship
- [ ] G1 Agent CI green on 3 OSes + manual smoke on real Linux / Mac / Windows
- [ ] G2 Phone x PC matrix, offline and signed in; revoke test
- [ ] G3 Failure modes (expired cert, account service down, clock skew, lost phone)
- [ ] G4 Final security + code review

## Needs a human (cannot be done from this environment)
T0.3, T3.5, T7.1 (if no Docker), T7.2, VPS provisioning and DNS, Play Console. Phase 8: Apple Developer Program + Developer ID/notarization, Windows signing cert, OAuth client IDs (Google/Apple), SMTP, real Mac / Linux desktop / iPhone testing, store accounts.

## Bug review (code-review, high)
Fixed: cmd.exe metacharacters accepted in `files_url` (agent, Android and Settings validators now identical, with tests); arm64 image build lacked QEMU; Kotlin port check accepted `+443`; Settings page validated more loosely than the app; HSTS typo; uvicorn access log contradicted the privacy wording; Files tab lost on activity recreate.
Not changed (upstream FileSync behaviour, worth an upstream issue): `signaling.py` does a blocking DNS lookup on the event loop and caches failures forever; the credentials rate limit is global behind a proxy (documented in DEPLOYING-SERVER.md).
