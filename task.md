# task.md — merge FileSync into Phone Remote

Decisions: **unified product** (one repo, one brand, in-app entry points) · FileSync server on **your own VPS**.
Source: https://github.com/polius/FileSync (MIT). Tick each box when the task is finished and verified.

## Phase 0 — Setup
- [x] T0.1 Create this `task.md`
- [ ] T0.2 Work on the existing branch; update PR #1 description (one branch, one PR)
- [ ] T0.3 Confirm ownership / licence of FileSync with the owner (MIT — attribution kept either way)

## Phase 1 — Import and CI
- [x] T1.1 Import FileSync into `filesync/` with `git subtree` (history squashed, LICENSE kept)
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
- [ ] T3.5 Local compose smoke test (needs Docker + two browsers) — partly done: app tested without Docker (health, uuid, credentials, WebSocket register OK; no Docker daemon in this environment)

## Phase 4 — Android "Files" tab
- [ ] T4.1 Third tab with a hardened WebView
- [ ] T4.2 Server URL via `BuildConfig`, https-only validation + unit test
- [ ] T4.3 File chooser and download handling
- [ ] T4.4 "Open in browser" fallback for large receives
- [ ] T4.5 Re-check permissions (none added)
- [ ] T4.6 Stretch: Android share-target

## Phase 5 — Windows agent entry points
- [ ] T5.1 Config key `files_url`
- [ ] T5.2 Dashboard card + tray item "Send files"
- [ ] T5.3 Stretch: "Send to this PC" QR
- [ ] T5.4 Rust tests

## Phase 6 — Website, docs, privacy, store
- [ ] T6.1 Landing page: "Send files" section
- [ ] T6.2 Rewrite privacy policy (a server now sees IPs/peer ids transiently)
- [ ] T6.3 Update Play Data safety answers in `docs/RELEASING.md`
- [ ] T6.4 Update RELEASING.md + guide page
- [ ] T6.5 Release checklist: server up and healthy before tagging

## Phase 7 — Verify and ship
- [ ] T7.1 FileSync e2e against local compose (needs Docker + Chromium)
- [ ] T7.2 Real-device matrix incl. cross-network (TURN relay)
- [ ] T7.3 `/security-review` of the new surface
- [ ] T7.4 CI green, PR ready, tag `v1.1.0` only after T3.5 and T7.2

## Needs a human (cannot be done from this environment)
T0.3, T3.5, T7.1 (if no Docker), T7.2, VPS provisioning and DNS, Play Console.
