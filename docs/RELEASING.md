# Releasing Lanyard

Everything ships from one command: `git tag v1.0.0 && git push --tags`.
`.github/workflows/release.yml` then builds, signs (if secrets exist) and publishes to GitHub Releases:

- `Lanyard-Setup-<version>.exe` — Windows installer
- `Lanyard-<version>-macos.dmg` — macOS disk image (universal: Apple silicon + Intel)
- `phone-remote_<version>-1_amd64.deb` / `_arm64.deb` and `phone-remote-<version>-linux-x86_64.tar.gz` / `-aarch64.tar.gz` — Linux
- `latest.json` and `SHA256SUMS` — read by the website, and for verifying downloads
- `Lanyard-<version>.apk` — Android APK (direct download)
- `Lanyard-<version>.aab` — Play Store bundle (workflow artifact only; needs the keystore secrets)
- `ghcr.io/malinanu/phonetodesktop-files:<version>` and `:latest` — the Send files server image (see [DEPLOYING-SERVER.md](DEPLOYING-SERVER.md))

Besides GitHub Releases, the `server` job of the same workflow copies every file to the website's own server, so the
download buttons on https://lanyard.3logiq.com point at files on your server:

- files go to `<SITE_SSH_PATH>/downloads/<version>/` (with `SHA256SUMS`); the newest 5 versions are kept, older ones are removed;
- `<SITE_SSH_PATH>/latest.json` is rewritten last, so the site never links to files that are not there yet;
- the site lists them on the inner page `downloads.html` (every file with size, SHA-256, install help; the Android APK is included), and
  the buttons on the home page link straight to the files. The GitHub release stays as a mirror ("Mirror on GitHub").

**Automatic builds:** every push to `main` that changes the programs (`agent/`, `android/`, `installer/`, `shared/`, `brand/`, or the
release workflow) builds all platforms and uploads them as version `0.3.<run number>` (for example `0.3.57`), so the website always has
downloads without anyone tagging anything. Those builds get no GitHub release and no server images (those are for tags and manual runs).
Changing only `site/` does not rebuild; it just redeploys the pages and leaves the downloads alone. Raise the `0.3` in the `TAG` line of
`release.yml` when a new minor version starts, and give real releases a higher version (for example `1.0.0`) than the automatic builds.

It uses the same five `SITE_SSH_*` secrets as the website deploy (see [DEPLOYING-WEBSITE.md](DEPLOYING-WEBSITE.md)) and does nothing when
they are not set. You can also start it without a tag: **Actions → release → Run workflow**, version `1.2.3`.
The App Store and Google Play buttons appear on the site when the repository variables `APP_STORE_URL` / `PLAY_STORE_URL` are set;
until then the page shows "Coming soon" for iPhone and the direct APK for Android.

Version numbers come from the tag (`v1.2.3` → versionName `1.2.3`, versionCode `1002003` = major×1000000 + minor×1000 + patch, so build numbers up to 999 never collide). Never reuse a tag.

## One-time setup

### 1. Android signing key  (required for Play and for updatable APKs)
```
keytool -genkeypair -v -keystore release.jks -alias phoneremote \
        -keyalg RSA -keysize 2048 -validity 10000
base64 -w0 release.jks     # macOS: base64 -i release.jks
```
Add these under repo **Settings → Secrets and variables → Actions**:

| Secret | Value |
| --- | --- |
| `ANDROID_KEYSTORE_B64` | the base64 output above |
| `ANDROID_KEYSTORE_PASSWORD` | keystore password |
| `ANDROID_KEY_ALIAS` | `phoneremote` |
| `ANDROID_KEY_PASSWORD` | key password |

**Back up `release.jks` and both passwords somewhere safe (password manager + offline copy). Never commit them.**
Lose the key and you can never ship an update under the same app identity. With Play App Signing (below) Google holds the
final signing key and you keep only an *upload* key, which Google can reset, but still back it up.

### 2. Windows code-signing certificate  (removes the SmartScreen warning)
Buy an OV or EV code-signing certificate (EV clears SmartScreen at once; OV builds reputation over time). Export it as a `.pfx`, then:
```
base64 -w0 cert.pfx
```
Add secrets `WIN_CERT_B64` (that output) and `WIN_CERT_PASSWORD`. The workflow signs the exe and the installer automatically.
Many EV certs ship on a hardware token and can't be exported; for those use a cloud signing service (Azure Trusted Signing,
SSL.com eSigner, etc.) and adjust the signing steps.

### 3. macOS signing and notarization (optional, removes the Gatekeeper warning)
Needs an Apple Developer Program membership ($99/yr). Without it the app is ad-hoc signed: it runs, but users must right-click → Open once, and macOS forgets the Accessibility permission after each update (the signature changes).
Create a **Developer ID Application** certificate, export it as `.p12`, then add these Actions secrets:

| Secret | Value |
| --- | --- |
| `MACOS_CERT_B64` | `base64 -i cert.p12` |
| `MACOS_CERT_PASSWORD` | the .p12 password |
| `APPLE_ID` | your Apple ID email |
| `APPLE_TEAM_ID` | the 10-character team id |
| `APPLE_APP_PASSWORD` | an app-specific password from appleid.apple.com |

The workflow signs with hardened runtime, submits the dmg to Apple's notary service, and staples the ticket.

### 4. Send files server and `FILES_URL`
Deploy the server first ([DEPLOYING-SERVER.md](DEPLOYING-SERVER.md)), then add the repository **variable** (not secret) `FILES_URL` = `https://files.example.com`
under Settings → Secrets and variables → Actions → Variables. Releases bake it into the Android app and the Windows agent, and the Pages workflow uses it for the website button.
Without it the Files tab asks for an address and the "Send files" buttons stay hidden.

### 5. Website (GitHub Pages)
Repo **Settings → Pages → Source: GitHub Actions**. `.github/workflows/pages.yml` publishes `site/` on every push to `main` (or the current default branch; consider renaming it to `main` under Settings → Branches).
The site lists every download from the release's `latest.json` and is rebuilt automatically when a release is published. Optional repository variables `APP_STORE_URL` (an `https://apps.apple.com/...` link) and `PLAY_STORE_URL` (`https://play.google.com/store/apps/details?id=...`) make the store buttons appear; until they are set the buttons stay hidden.
The privacy policy URL for Play is `https://<owner>.github.io/<repo>/privacy.html` (or your custom domain, set under Pages).

## Google Play
1. Create a developer account at play.google.com/console ($25 one-time; new personal accounts must run a closed test with
   12+ testers for 14 days before production access).
2. Create the app, then fill in: privacy policy URL, **Data safety** form, content rating, target audience, app category (Tools).
   Data safety answers: *no data collected, no data shared*. This holds because the app itself contacts no server of yours: remote control is phone-to-PC on the LAN,
   and the Files tab only launches the phone's browser for Send files. The privacy policy still describes the file service (see `site/privacy.html`).
   **Re-answer the form if you ever load the Send files page inside the app (WebView), add analytics or crash upload, or send anything else off the device.**
3. Enrol in **Play App Signing** when uploading the first bundle, and upload `Lanyard-<version>.aab` from the release workflow run
   (Actions → run → Artifacts → `android-aab`).
4. Permissions Play may ask you to justify: Bluetooth connect/advertise (Bluetooth HID remote mode) and local network discovery (finding the PC).
5. Roll out: internal test → closed test → production. Every upload needs a higher versionCode, which a new tag gives you.

## Store forms for the lock-screen controls
- **Google Play, App content > Foreground service permissions:** the app declares `FOREGROUND_SERVICE_CONNECTED_DEVICE`
  (`MediaControlsService`). Describe it as: *"Shows play/pause/skip controls for the user's paired computer in the
  notification and on the lock screen, and keeps the connection to that computer while the phone is locked. Starts
  when the user connects to their computer; the user can turn it off in Settings."* Attach a short screen recording
  of the notification controlling the computer.
- **App Store review notes:** the `audio` background mode is used only by Settings > *Full lock-screen player*
  (off by default), which keeps an audio session so iOS shows its lock-screen player for the computer. The Live
  Activity and Control Center buttons need no background mode. If review rejects the background audio, remove
  `audio` from `UIBackgroundModes` in `mobile/ios/Runner/Info.plist` and hide that switch; the rest keeps working.
- **iPhone signing:** enable the *Keychain Sharing* capability for `app.phoneremote` and `app.phoneremote.widgets`
  with the group `app.phoneremote.shared` (the entitlements files already list it), so the Control Center buttons can
  read the login details the app stores.

## Release checklist
- [ ] CI green on `main` (`build` and `filesync-checks` workflows)
- [ ] Send files server deployed, `https://<files-domain>/api/health` returns 200, and `FILES_URL` is set
- [ ] A file sent between two devices on different networks (forces the TURN relay) arrives intact
- [ ] Smoke-tested the macOS dmg on a real Mac (opens, Accessibility prompt, media keys) and the Linux .deb on a real desktop
- [ ] Installed the APK on a real phone; Wi-Fi pairing and Bluetooth mode both work
- [ ] Ran the Windows installer on a real PC; tested with VLC, a browser and Spotify
- [ ] Tag pushed, release published, download links on the site work
