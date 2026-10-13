# Accounts (optional)

Lanyard works with **no account**: scan the QR, approve on the computer, done, entirely on your own network.
An account is a convenience layer on top, run by you (`account/`, a small FastAPI service):

- a list of **your devices** (phones and computers) with names, so you can see and remove them from one place;
- **device certificates**: short-lived signed statements "this phone belongs to account X". A computer that trusts
  the account's public key accepts any of your phones **without a QR code**, and keeps doing so with **no
  internet**, because checking a certificate is just checking a signature;
- **revocation**: removing a device stops its certificate being renewed and puts it on a signed list the
  computers fetch whenever they are online.

Nothing about media control or files passes through the account service, and local pairing keeps working if it is
down. It stores: your email address, your devices (name, platform, public key), hashed session tokens, and (for
at most 15 minutes) a hashed sign-in code. No passwords. Deleting the account (`DELETE /v1/me`) removes all of it.

## Trust model
```
account service ──signs──▶ device certificate ──presented by──▶ phone ──▶ computer checks signature offline
```
- The computer learns the service's **public keys** once (from `/.well-known/phoneremote.json`) and its own account
  id when it joins. Those are cached on the computer.
- A certificate lasts **30 days** (`ACCOUNT_CERT_DAYS`); the phone app renews it whenever it is online. A computer
  accepts a certificate up to a configurable **grace period after expiry** so a phone that was away a while still
  works. Revocation reaches a computer when it is next online: until then a removed phone's still-valid
  certificate is accepted. Keep certificate life short if that matters to you.
- The certificate only says *which key belongs to which account*; the phone still proves it holds that key by
  signing the local login challenge (see [PROTOCOL.md](PROTOCOL.md)).

## Using it
1. **Phone**: Settings, Account (or "I have an account" on the first screen): type your email, then the code you
   receive. The phone enrols its existing device key (the private key never leaves the phone) and saves the
   certificate and a refresh token in the OS keystore. It renews the certificate when under 7 days are left.
2. **Computer**: on the website's [My devices](https://lanyard.3logiq.com/devices.html) page choose *Get a code*;
   in the computer's dashboard, Settings, Account (optional), type the service address and the code. The computer
   enrols itself as a device, publishes how phones find it (its id and the fingerprint of its TLS key) and caches
   the service keys. It then asks for the removal list at start and every 15 minutes (*Check now* does it at once).
3. **Phone**: Settings, Account, *Find my computers*. Computers of the account that are on this Wi-Fi are added,
   pinned to the key fingerprint the account holds. From then on the phone logs in with its certificate and no QR
   code is needed. Without the account, scanning a QR code still works exactly as before.

The phone learns a computer's TLS fingerprint from the account over HTTPS, not from the local network, so a
device on your Wi-Fi cannot pretend to be your computer by advertising a different key.

## Threat model
What the design protects against, and what it does not.

| Situation | What happens |
| --- | --- |
| Someone on your Wi-Fi tries to control a computer | Needs a device key the computer knows (paired by QR, or an account certificate **and** the matching private key). A certificate alone is useless: login also signs a per-connection challenge. |
| Someone on your Wi-Fi impersonates your computer | The phone pins the computer's TLS key (from the QR, or from the account). A different key is refused ("keyChanged"). |
| A phone is lost or stolen | Remove it on the My devices page. Computers that are online learn within about 15 minutes (poll interval) and drop its live connection; a computer that is **offline** accepts the old certificate until it expires (at most 30 days) plus the grace period (14 days) unless it checks the list earlier. If that gap matters: lower `ACCOUNT_CERT_DAYS`, lower the grace, and use *Remove all phones* on the computer (that only affects QR-paired phones; leave the account to cut account phones off at once). The phone's own screen lock and the OS keystore are the first line of defence. |
| The account service is down | Nothing local stops working: certificates are checked offline, QR-paired phones are unaffected, the removal list keeps its last copy. Only sign-in, enrolling new devices and renewing certificates wait. |
| The account service is hacked | The attacker can mint certificates for any account, so any computer that joined can be controlled by whoever also reaches it on the local network and holds a minted key. Mitigations: keys only in the server's `.env`, rotate (add `k2` first, drop `k1`), computers can leave the account (Settings, Account, *Leave*) which also drops every account phone. A phone cannot be controlled remotely over the internet: the control port is only reachable on the local network. |
| A fake removal list is injected | It is signed with the service key and bound to the account id, so a list from anyone else (or another account) is ignored. An attacker who can block the poll can only delay revocation, not forge it. |
| Clocks are wrong | A certificate "issued" more than 5 minutes in the future is refused; a computer whose clock is far ahead sees certificates as expired, one far behind sees them as not yet valid. Fixing the clock fixes it; QR-paired phones do not depend on the clock. |
| Someone guesses a sign-in or link code | Sign-in codes: 5 wrong tries burn the code, per-IP and per-address rate limits. Link codes: 10-minute life, single use, rate limited. The revocation URL contains the unguessable account id and discloses only which device ids were removed. |
| The service address is a malicious one | Typed by the owner on the computer or phone. Using someone else's service would let them mint certificates for the computer; only join an account service you run or trust. |

Not covered: a stolen **unlocked** phone, malware on the phone or computer, and a computer owner who approves a
phone they should not. The account does not add end-to-end protection for the media data itself; that is the pinned
TLS connection.

## Certificate format
`cert = base64url(payload) + "." + base64url(signature)`; the signature is Ed25519 over
`"PRv2-cert" 0x00 || payload_bytes`, made by the service key named in the payload's `kid`. Verify the **exact
bytes** received (no JSON re-encoding), then check the fields.

| field | meaning |
| --- | --- |
| `v` | `1` |
| `kid` | which service key signed it (keys can be rotated) |
| `acct` | account id (random, 32 hex) |
| `dev` | device id, the same id the device uses on the local protocol |
| `kind` | `phone` or `pc` |
| `name` | device name for display |
| `pk` | the device's Ed25519 public key, base64url |
| `iat`, `exp` | unix seconds |

The revocation list is a signed envelope `{payload, sig, kid}` (prefix `"PRv2-signed" 0x00`); its payload is
`{"acct", "now", "revoked": [{"dev", "at"}]}`.

## Endpoints
| | |
| --- | --- |
| `POST /v1/auth/email` `{email}` | emails a sign-in code (same answer for any address) |
| `POST /v1/auth/verify` `{email, code}` | exchanges the code for tokens; 5 wrong tries burn the code |
| `POST /v1/auth/oidc` `{provider: google\|apple, id_token, nonce?}` | Sign in with Google / Apple |
| `POST /v1/auth/refresh` / `logout` | rotating refresh tokens; reusing an old one ends the session |
| `GET`/`DELETE /v1/me` | the account; delete removes everything |
| `POST /v1/devices` `{id, name, platform, kind, pubkey}` | enrol a device, returns its certificate |
| `GET /v1/devices`, `PATCH`/`DELETE /v1/devices/{id}`, `POST /v1/devices/{id}/cert` | list, rename, remove, renew |
| `POST /v1/links` / `POST /v1/links/claim` | a signed-in user makes a one-time code; a computer with no login screen joins with it |
| `GET /v1/accounts/{id}/revocations` | signed list of removed devices (the account id is unguessable) |
| `GET /.well-known/phoneremote.json` | the service's public keys |

## Running it
```
# keys (keep these safe; rotate by adding a new first key and keeping the old one until its certificates expire)
python3 -c "import os,base64;print('k1:'+base64.urlsafe_b64encode(os.urandom(32)).rstrip(b'=').decode())"
python3 -c "import secrets;print(secrets.token_urlsafe(48))"          # ACCOUNT_JWT_SECRET
```
Put them, a database password and (optionally) SMTP and Google/Apple settings in `deploy/.env` (see
`.env.example`), then `docker compose --profile accounts up -d`. The service is then at
`https://<your files domain>/account/`. Without SMTP settings sign-in codes are only written to the log
(development); real use needs SMTP. Google and Apple sign-in turn on when `GOOGLE_CLIENT_IDS` /
`APPLE_CLIENT_IDS` are set. For local development: `ACCOUNT_DEV_MODE=1 uvicorn app.main:create_app --factory`.

## Limits worth knowing
- Rate limits are in memory per process; behind several replicas, add a shared limiter in front.
- The service holds the signing keys: whoever controls them can mint certificates for any account. Protect the
  `.env`, back it up, and treat a leak as a reason to rotate (add a new `k` first, redeploy, then drop the old).
