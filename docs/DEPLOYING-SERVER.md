# Deploying the "Send files" server

Phone Remote itself needs no server. Only **Send files** (the imported FileSync app in `filesync/`) does:
a small signaling server helps two devices find each other, and a TURN relay (coturn) carries the encrypted
traffic when a direct connection is impossible. File bytes are never stored on the server.

## What you need
- A VPS with a public IPv4 address: 1 vCPU / 1 GB RAM is enough to start; Linux with Docker + Docker Compose v2.
- A domain you control, e.g. `files.example.com`.

## 1. DNS
Create an `A` (and `AAAA` if you have IPv6) record for `files.example.com` pointing at the VPS.
It **must** point at this same server: FileSync rewrites relay ICE candidates to the address the client used to reach it, and coturn runs on this host.

## 2. Firewall
Open on the VPS and in the provider's firewall:

| Port | Protocol | Purpose |
| --- | --- | --- |
| 80 | TCP | certificate challenge + redirect to HTTPS |
| 443 | TCP + UDP | web app (HTTPS, HTTP/3) |
| 3478 | TCP + UDP | STUN/TURN |
| 50000-50100 | UDP | TURN relay range |

## 3. Start it
```
git clone https://github.com/malinanu/phonetodesktop && cd phonetodesktop/deploy
cp .env.example .env        # set FILES_DOMAIN, optionally FILES_TAG
docker compose up -d
docker compose ps            # all running; "init" exits 0 after creating the secret
curl -fsS https://files.example.com/api/health
```
The image is `ghcr.io/malinanu/phonetodesktop-files`, built and pushed by the release workflow on every `v*` tag.
Make the package public once (GitHub → Packages → the image → Package settings → Change visibility), or `docker login ghcr.io` on the server.
Before the first release exists, build locally instead: `docker build -t ghcr.io/malinanu/phonetodesktop-files:latest filesync`.

## 4. Point the apps at it
- **Release builds (recommended):** set one repository variable, GitHub → Settings → Secrets and variables → Actions → *Variables* → `FILES_URL` = `https://files.example.com`. The release workflow bakes it into the Android app (`-PfilesUrl`) and the Windows agent (`PHONE_REMOTE_FILES_URL`).
- Local Android build: `gradle -p android assembleDebug -PfilesUrl=https://files.example.com`. Users can also type their own address in the app under Settings → Send files.
- Windows agent: users can override with `"files_url": "https://..."` in `%APPDATA%\phone-remote\config.json`. Empty and no build default = the "Send files" buttons are hidden.
- Website: set the link in `site/index.html` (Send files section) to the same URL.

## Operating it
- **Upgrade:** `docker compose pull && docker compose up -d`. Pin `FILES_TAG=1.2.3` to control upgrades.
- **Back up:** only the `keys` volume matters (the signing secret). Losing it just means a new secret; nothing else is stored. `caddy-data` holds the certificate and is re-issued automatically.
- **Monitor:** alert on `https://<domain>/api/health` (any uptime service) and on outbound bandwidth: TURN relays only the minority of connections that can't go direct, but it is the one thing that can cost money.
- **Logs:** `docker compose logs -f filesync coturn`. The app logs connections, not file contents.

## Security notes
- Only 80/443 (web + WebSocket + `/api/*`) and the TURN ports are exposed. The signing secret is shared only through the internal `keys` volume.
- coturn denies relay to private address ranges, so it can't be used to reach the host's internal network, and per-user/total allocation quotas are set in `deploy/docker-compose.yml`. Add `--max-bps` / `--bps-capacity` if bandwidth abuse appears.
- `/api/credentials` mints short-lived TURN credentials and is public by design. nginx rate-limits it, but behind Caddy every request arrives from Caddy's container address, so the limit works as a **global** backstop (20 req/s, burst 40), not per client. That is fine for normal use. If you need per-client limits, add `set_real_ip_from <caddy subnet>; real_ip_header X-Forwarded-For;` to `filesync/nginx.conf`.
- Signaling is in-memory and single-worker. Don't scale the app container beyond one replica.

## Troubleshooting
- Transfers work on one network but not across networks: UDP 3478 / 50000-50100 blocked, or DNS doesn't point at this server.
- Certificate not issued: port 80 closed, or DNS not propagated yet (`docker compose logs caddy`).
- Receiver on iOS/Android browser memory-limited on huge files: serve over HTTPS (required for streamed downloads).
