# Accounts (optional)

Phone Remote works with **no account**: scan the QR, approve on the computer, done, entirely on your own network.
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
