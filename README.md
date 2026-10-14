# Lanyard

**Lanyard** ties your phone to your computer: a remote, touchpad, keyboard and file sender, over your own Wi-Fi (or Bluetooth on Android). Free, open source, private, no account needed.

Website: https://lanyard.3logiq.com · [Privacy policy](site/privacy.html) · [Releases](https://github.com/malinanu/phonetodesktop/releases/latest)

| | Windows | macOS | Linux | Android | iPhone |
| --- | --- | --- | --- | --- | --- |
| Computer app | installer (`.exe`) | disk image (`.dmg`, universal) | `.deb`, `.tar.gz` | | |
| Phone app | | | | APK (Flutter app and native app), Play Store later | Flutter app builds in CI; App Store needs the owner's Apple account |

## How it works
1. Install Lanyard on the computer and open it. Its window shows a QR code.
2. Open the phone app, tap **Scan the code on my computer**, point the camera at it.
3. Click **Allow** on the computer. Done: it stays paired until you remove the phone.

The phone talks directly to the computer on your Wi-Fi; there is no Lanyard server in the middle. Without Wi-Fi, an Android phone can act as a **Bluetooth** keyboard and media remote for any computer, with nothing installed on it.

## What it does
- **Media remote:** play, pause, skip, seek and volume for the player on the computer (title and progress on Windows and Linux; on macOS the media keys and volume work, but titles are not available).
- **Touchpad and keyboard:** move, click, scroll and type on the computer. Each phone has its own "mouse and keyboard" permission.
- **Send files:** built in, no setup (see below).
- **Bluetooth mode (Android):** keyboard, mouse and media keys over Bluetooth, no software on the computer.
- **Optional account** (`account/`, not required, not deployed by default): a list of your devices and device certificates a computer can check offline. See [docs/ACCOUNTS.md](docs/ACCOUNTS.md).

## Send files (built in)
The computer program serves the [FileSync](https://github.com/polius/FileSync) page and its small signaling relay on the home network (agent port + 1, `agent/src/files.rs`; the web app is embedded at build time by `agent/build.rs`). Click **Send files** on the computer, scan its code from the phone's **Files** tab, pick files; they go directly between the two devices over encrypted WebRTC with no size limit when the computer receives. A phone receiving over plain Wi-Fi has to hold the file in memory (browsers only stream to disk on HTTPS or localhost), so keep files sent *to a phone* to a few hundred MB.

Sending between **different networks** (over the internet) needs your own server (FileSync + TURN): optional, see [docs/DEPLOYING-SERVER.md](docs/DEPLOYING-SERVER.md) (Caddy stack, or Portainer + CloudPanel on Oracle Cloud). Enter its address in the apps (or set the `FILES_URL` repo variable for builds).

## Repository layout
```
agent/     Rust desktop agent (Windows, macOS, Linux): phone UI, WebSocket protocol, dashboard, tray, built-in Send files
shared/    Design tokens (base.css, font), the phone remote pages and the in-app Guide, shared by the agent and the native Android app
mobile/    Flutter app for Android and iOS (native remote, touchpad, Files, Bluetooth on Android, QR pairing)
android/   Native Android app (kept until the Flutter app reaches parity): Wi-Fi mode in a WebView, Bluetooth HID mode
filesync/  FileSync (imported from github.com/polius/FileSync, MIT): web app, signaling server and tests for internet use
account/   Optional account service (FastAPI): sign-in, devices, signed device certificates, revocation list
deploy/    Optional server stacks: Caddy, Portainer, and Portainer behind CloudPanel (no Caddy)
site/      The website, deployed to lanyard.3logiq.com
brand/     The mark (brand/geometry.mjs) and the script that renders every icon from it
installer/ Windows installer (Inno Setup), macOS .dmg, Linux .deb / tarball scripts, release manifest (latest.json)
docs/      PROTOCOL, ACCOUNTS, RELEASING, DEPLOYING-SERVER, DEPLOYING-WEBSITE, DESKTOP-TESTING
.claude/   Design skills installed for working on the website and UI (taste-skill, impeccable, ui-ux-pro-max)
```

## Desktop agent
The same Rust agent runs on all three systems. Install from the website or a release. On macOS and Linux open it with `phone-remote open` (or the applications menu); `phone-remote autostart on` starts it at login. On Windows, `phone-remote install` (as Administrator, once) adds start-at-login and the private-network firewall rule; the installer does this for you. `phone-remote rotate` removes every paired phone, `--mock` runs a fake player for development, `--port N` changes the port.

The dashboard (double-click the tray icon, or http://127.0.0.1:8765/dashboard) is one screen with the next step: connect your phone, waiting, or connected. Phones, Help and, under *More*, Video players, Settings and Activity are one click away.

Platform notes: Windows reads the media session (GSMTC) and falls back to arrow keys for apps that ignore seeking; Linux reads players over MPRIS and sends input through X11 (Wayland needs a one-time permission); macOS needs the Accessibility and Local Network permissions. Details: [docs/DESKTOP-TESTING.md](docs/DESKTOP-TESTING.md).

## Security
The QR carries a 15-minute pairing code that only lets a phone *ask* to join, plus the fingerprint of this computer's key. The computer owner approves, and the phone is then known by its **device key** (the computer stores only the public key). Every login signs a fresh per-connection challenge, so nothing can be replayed. Phone traffic runs over **HTTPS with a certificate the phone pins**, so other people on the same Wi-Fi cannot read or change it. Also: lockout after 5 failures a minute, WebSocket origin check, LAN-only source addresses, and a loopback-only dashboard with Host-header and custom-header checks. Phones from before the key-based protocol still work until you switch them off (Settings, *Allow old versions of the phone app*). Send files is LAN-only, can be switched off in Settings, and the relay only passes connection set-up messages. Protocol: [docs/PROTOCOL.md](docs/PROTOCOL.md).

## Build and test
```
cd agent && cargo test                       # unit tests, including the Send files relay
cd mobile && flutter analyze && flutter test # app, widget tests, and live tests against the real agent binary (build it first)
cd account && python -m pytest -q            # account service
cd installer && python -m unittest test_make_latest
gradle -p android testDebugUnitTest assembleDebug   # native Android app
cd filesync && npm ci && npm test            # FileSync web tests; e2e/ drives real browsers (also against the built-in server)
```
Icons: `CHROMIUM_PATH=... node brand/make-icons.mjs` regenerates every icon (needs Node 22, `playwright`, Chromium, ImageMagick).

CI (`.github/workflows`): `build.yml` tests and builds every platform on each push (Windows agent, Linux and macOS agents with packaging, Android APK, Flutter APK and unsigned iOS build, account and FileSync checks); `release.yml` publishes installers, `latest.json` and checksums when a `v*` tag is pushed (or when started by hand with a version) and copies the files to the website server, which serves them from `lanyard.3logiq.com/downloads.html`; `deploy-site.yml` uploads the website to CloudPanel over SSH. See [docs/RELEASING.md](docs/RELEASING.md).

## Status
Verified in CI and here: agent, relay, Flutter app and account service tests; the builds for Windows, macOS, Linux, Android and unsigned iOS; Send files transfers (Chromium, 20 to 50 MiB, SHA-256 checked) through the built-in server; the website in a real browser at desktop and phone widths.

Not yet verified on real hardware or servers: a phone on Wi-Fi with a Windows or macOS computer (the firewall prompt, Send files from the phone's browser), macOS media keys and Accessibility, Linux on Wayland, the Android Bluetooth mode, the Windows exe icon embedding, and the website, Portainer and TURN deployment on the Oracle server. Release signing (Windows certificate, Apple Developer ID and notarization, Play and App Store accounts) needs the owner's credentials. The remaining plan and checklist are in [task.md](task.md).

## Licence
[MIT](LICENSE). Send files is built on [FileSync](https://github.com/polius/FileSync) by polius (MIT, see `filesync/LICENSE`); the comic on its About page is xkcd #949 (CC BY-NC 2.5).
