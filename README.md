# Phone Remote

Control media on a Windows PC from an Android phone, over the local network (Wi-Fi) or Bluetooth.

```
agent/     Rust desktop agent (Windows GSMTC backend), serves the phone UI + WebSocket
android/   Android app: Wi-Fi mode (WebView + mDNS discovery + QR pairing) and Bluetooth HID mode
```

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

## Security
256-bit secret from the QR, constant-time compare, 5-failures/min lockout, WebSocket Origin check,
private/LAN source addresses only, `/pair` only from localhost. Traffic is plain HTTP on the LAN.

## Status
Tested: controller logic (unit tests), WebSocket auth/commands/Origin check against the mock backend on Linux.
Compiles (not run): the Windows backend (`cargo check --target x86_64-pc-windows-gnu`).
Not built or run: the Android app (no Android SDK available when written). CI builds both.
