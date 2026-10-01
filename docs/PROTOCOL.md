# Phone ⇄ computer protocol

Everything is one WebSocket at `/ws` on the computer's agent (default port 8765), JSON text frames, each with a
`"t"` (type) field. Commands from a logged-in phone are `{"c": "...", ...}` objects (see `agent/src/protocol.rs`).

Two login styles exist. **v2 (device keys) is the one to implement.** v1 (bearer tokens) remains only so phones
that were paired before keys existed keep working; the owner can switch it off in the dashboard
(Settings → *Allow older phone apps*), after which only v2 phones connect.

## Device identity (v2)
- Every phone generates an **Ed25519** key pair once and keeps the private key in the OS keystore
  (Android Keystore / iOS Keychain). The computer stores only the **public key**; it never holds a secret that
  could be used to impersonate the phone.
- `device` is a random id chosen by the phone (≤ 64 chars), stable across IP changes. `pk` is the 32-byte public
  key, base64url without padding.

## Pairing (first contact, from the QR code)
The QR carries `…#k=<pairing code>&id=<pc id>&n=<pc name>`. The code is short-lived (15 min) and only lets a phone
*ask* to join; the owner must approve on the computer.

```
phone → pc   {"t":"pair","code":"<k>","device":"<id>","name":"Pixel 8","pk":"<base64url>","platform":"android"}
pc → phone   {"t":"pair","status":"pending"}               (owner sees an approval window)
pc → phone   {"t":"pair","status":"approved","v":2}        (no token: nothing secret is handed out)
             or "denied" | "expired" | "bad_code" | "bad_key"
```
Re-pairing a known device with the **same** key is silent. A **different** key under a known id is treated as a
new device and needs approval again, so a QR code cannot be used to take over an existing phone.

## Login (every connection)
```
phone → pc   {"t":"challenge","device":"<id>"}
pc → phone   {"t":"challenge","nonce":"<32 random bytes, base64url>"}
phone → pc   {"t":"auth_sig","device":"<id>","sig":"<base64url Ed25519 signature>"}
pc → phone   {"t":"auth","ok":true,"input":<bool>}          or {"t":"auth","ok":false,"err":"..."}
```
The phone signs these exact bytes (`auth_message` in `agent/src/auth.rs`):

```
"PRv2-auth" 0x00 <pc id, UTF-8> 0x00 <device id, UTF-8> 0x00 <nonce, the raw 32 bytes>
```
- The **nonce** is fresh for each connection, so a recorded login cannot be replayed.
- The **pc id** (from the QR / mDNS) binds the signature to this computer, so it is useless against another one.
- Errors: `"revoked"` (the computer no longer knows this device: forget it and pair again), `"bad token"` (signature
  did not verify; counts towards a lockout of 5 failures per minute), `"locked"`, `"v2_required"` (this computer
  refuses v1 phones).

## v1 (older phones, being phased out)
`{"t":"auth","token":"<device token>","device":"<id>"}` after pairing with `{"t":"pair","code":…,"device":…,"name":…}`
(no `pk`); the approval message then carries `device_token`. Plain bearer token over plain HTTP.

## Transport (HTTPS with a pinned key)
The agent listens on one port and speaks **both** TLS and plain HTTP (it looks at the first byte of each connection).

- Each computer makes one self-signed ECDSA P-256 certificate on first run (`tls-key.der`, `tls-cert.der` in its
  config folder, key file readable only by the owner). The key never changes; only the certificate is renewed, long
  before it expires, so the fingerprint below stays valid.
- The QR payload is `http://<ip>:<port>/#k=<code>&id=<pc id>&n=<name>&fp=<fingerprint>`. It stays an `http://` link so
  a plain camera app can still open the browser remote while older phones are allowed. **`fp`** is the
  base64url (no padding) SHA-256 of the certificate's **SubjectPublicKeyInfo** (the same value as
  `curl --pinnedpubkey sha256//…` once converted to standard base64).
- A phone connects to `wss://<ip>:<port>/ws` and accepts the certificate **only if** the SHA-256 of its
  SubjectPublicKeyInfo equals `fp`. Ignore the host name, validity dates and certificate chain; still verify the
  TLS handshake signature as usual (proof that the server holds the key). TLS 1.2 and 1.3 are offered, ALPN `http/1.1`.
- Plain HTTP is always served to this computer itself (the dashboard). From the network it is served only while
  the owner leaves *Allow older phone apps* on; otherwise the agent answers with a short "install the secure app"
  notice and closes the connection.
- Device keys (above) are still how a phone proves who it is; TLS stops anyone on the Wi-Fi from reading or
  changing the traffic.
