# Releasing Phone Remote

Everything ships from one command: `git tag v1.0.0 && git push --tags`.
`.github/workflows/release.yml` then builds, signs (if secrets exist) and publishes to GitHub Releases:

- `PhoneRemote-Setup-<version>.exe` — Windows installer
- `PhoneRemote-<version>-macos.dmg` — macOS disk image (universal: Apple silicon + Intel)
- `phone-remote_<version>-1_amd64.deb` / `_arm64.deb` and `phone-remote-<version>-linux-x86_64.tar.gz` / `-aarch64.tar.gz` — Linux
- `latest.json` and `SHA256SUMS` — read by the website, and for verifying downloads
- `PhoneRemote-<version>.apk` — Android APK (direct download)
- `PhoneRemote-<version>.aab` — Play Store bundle (workflow artifact only; needs the keystore secrets)
- `ghcr.io/malinanu/phonetodesktop-files:<version>` and `:latest` — the Send files server image (see [DEPLOYING-SERVER.md](DEPLOYING-SERVER.md))

Version numbers come from the tag (`v1.2.3` → versionName `1.2.3`, versionCode `10203`). Never reuse a tag.

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
3. Enrol in **Play App Signing** when uploading the first bundle, and upload `PhoneRemote-<version>.aab` from the release workflow run
   (Actions → run → Artifacts → `android-aab`).
4. Permissions Play may ask you to justify: Bluetooth connect/advertise (Bluetooth HID remote mode) and local network discovery (finding the PC).
5. Roll out: internal test → closed test → production. Every upload needs a higher versionCode, which a new tag gives you.

## Release checklist
- [ ] CI green on `main` (`build` and `filesync-checks` workflows)
- [ ] Send files server deployed, `https://<files-domain>/api/health` returns 200, and `FILES_URL` is set
- [ ] A file sent between two devices on different networks (forces the TURN relay) arrives intact
- [ ] Smoke-tested the macOS dmg on a real Mac (opens, Accessibility prompt, media keys) and the Linux .deb on a real desktop
- [ ] Installed the APK on a real phone; Wi-Fi pairing and Bluetooth mode both work
- [ ] Ran the Windows installer on a real PC; tested with VLC, a browser and Spotify
- [ ] Tag pushed, release published, download links on the site work
