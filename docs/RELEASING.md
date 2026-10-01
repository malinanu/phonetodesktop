# Releasing Phone Remote

Everything ships from one command: `git tag v1.0.0 && git push --tags`.
`.github/workflows/release.yml` then builds, signs (if secrets exist) and publishes to GitHub Releases:

- `PhoneRemote-Setup-<version>.exe` — Windows installer
- `PhoneRemote-<version>.apk` — Android APK (direct download)
- `PhoneRemote-<version>.aab` — Play Store bundle (workflow artifact only; needs the keystore secrets)

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

### 3. Website (GitHub Pages)
Repo **Settings → Pages → Source: GitHub Actions**. `.github/workflows/pages.yml` publishes `site/` on every push to `main`.
The privacy policy URL for Play is `https://<owner>.github.io/<repo>/privacy.html` (or your custom domain, set under Pages).

## Google Play
1. Create a developer account at play.google.com/console ($25 one-time; new personal accounts must run a closed test with
   12+ testers for 14 days before production access).
2. Create the app, then fill in: privacy policy URL, **Data safety** form, content rating, target audience, app category (Tools).
   Data safety answers: *no data collected, no data shared* (see `site/privacy.html`; the app has no analytics and no external servers).
3. Enrol in **Play App Signing** when uploading the first bundle, and upload `PhoneRemote-<version>.aab` from the release workflow run
   (Actions → run → Artifacts → `android-aab`).
4. Permissions Play may ask you to justify: Bluetooth connect/advertise (Bluetooth HID remote mode) and local network discovery (finding the PC).
5. Roll out: internal test → closed test → production. Every upload needs a higher versionCode, which a new tag gives you.

## Release checklist
- [ ] CI green on `main` (`build` workflow)
- [ ] Installed the APK on a real phone; Wi-Fi pairing and Bluetooth mode both work
- [ ] Ran the Windows installer on a real PC; tested with VLC, a browser and Spotify
- [ ] Tag pushed, release published, download links on the site work
