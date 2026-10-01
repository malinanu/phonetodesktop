# Phone Remote

Control media on a Windows PC from an Android phone, over the local network (Wi-Fi) or Bluetooth.

```
agent/     Rust desktop agent (Windows GSMTC backend, tray app), serves the phone UI + WebSocket
shared/    Design system (base.css, font) and the in-app Guide page, used by both the PC agent and the Android app
android/   Android app: Wi-Fi mode (WebView + mDNS discovery + QR pairing) and Bluetooth HID mode
```

## Download and releasing
Downloads, privacy policy and the landing page live in `site/` (published with GitHub Pages). Releases are cut by tagging; see [docs/RELEASING.md](docs/RELEASING.md) for signing, Play Store and website setup. Licensed under [MIT](LICENSE).

## Settings and themes
The remote's **⋮ menu → Settings** (and the same page from the Bluetooth tab): theme (System / Dark / Light), skip step, volume step, touchpad speed, scroll direction, tap-to-click, vibration and keep-screen-on. In the Android app the values live in the app and are shared by every page; in a plain browser they live in `localStorage`. Colours are defined once in `shared/base.css` (and mirrored in `Theme.kt`) and checked for 4.5:1 text contrast in both themes.

## Guide pages
`shared/guide.html` holds two pages, *Get connected in 60 seconds* and *What happens when you tap play*. The PC serves it at `/guide` (tray menu → Guide) and the Android app bundles it (top bar → Guide).

## Windows agent
Download `phone-remote.exe` from the CI artifacts (or `cargo build --release` in `agent/`), then:

```
phone-remote install   # start at login + private-network firewall rule (run as Administrator once)
phone-remote           # prints a pairing QR; also http://127.0.0.1:8765/pair
```
Scan the QR with the phone camera (opens the controller in Chrome) or with the Android app.
`phone-remote rotate` revokes every paired phone. `--mock` runs a fake player for development.

## How commands are delivered
Play/pause/next/prev go to the media session (GSMTC). Seek is absolute via `TryChangePlaybackPosition`,
then verified ~350 ms later; if the position did not move (some apps return `true` and ignore it) the app is
remembered as broken and Left/Right arrows are sent to the foreground window if it is a known player
(VLC, mpv, MPC, browsers — table in `agent/src/backend/windows.rs`). Volume uses the system media keys.

## Bluetooth
The Android app's Bluetooth mode makes the phone a Bluetooth keyboard/media-key device (API 28+).
Pair it once from Windows (Settings → Bluetooth → Add device), press *Make phone discoverable* in the app first.
No agent needed, but there is no now-playing display, and seek is arrow keys to the focused window.

## Windows SmartScreen
Unsigned downloads show "Windows protected your PC / Unknown publisher". Click **More info → Run anyway** (or right-click the file → Properties → Unblock). The release workflow signs the agent exe and the installer automatically when the `WIN_CERT_B64` and `WIN_CERT_PASSWORD` secrets are set; a signed build removes the warning.

## Security
The QR carries a 15-minute pairing code that only lets a phone *ask* to join; the PC owner approves it, and the phone then receives its own revocable key. Constant-time compares, 5-failures/min lockout, WebSocket Origin check, LAN-only source addresses. The dashboard and its API are loopback-only with Host-header and custom-header checks (DNS-rebinding/CSRF). Traffic is plain HTTP on the LAN.

## Dashboard
Double-click the tray icon: Home, Phones (approve/remove, QR), Video players (one-click setup), Settings, Activity, Help. It opens as an app-style Edge window.

## Status
Tested: controller logic (unit tests), WebSocket auth/commands/Origin check against the mock backend on Linux.
Compiles (not run): the Windows backend (`cargo check --target x86_64-pc-windows-gnu`).
Not built or run: the Android app (no Android SDK available when written). CI builds both.
