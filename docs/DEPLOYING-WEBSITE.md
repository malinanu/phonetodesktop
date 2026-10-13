# Deploying the website (lanyard.3logiq.com on CloudPanel)

The site is plain static files (`site/`), so CloudPanel only has to serve a folder. GitHub Actions
(`.github/workflows/deploy-site.yml`) uploads it over SSH on every change and after every release, and always keeps a
`lanyard-site.zip` artifact for uploading by hand.

## One-time setup
1. **DNS.** `A` record `lanyard.3logiq.com` to the server's public IP.
2. **CloudPanel.** Sites, Add Site, **Create a Static Site**, domain `lanyard.3logiq.com`. CloudPanel creates a site user
   and the folder `/home/<site user>/htdocs/lanyard.3logiq.com`. Then SSL/TLS, New Let's Encrypt Certificate.
3. **An SSH key for deploys.** On your own machine: `ssh-keygen -t ed25519 -C "lanyard-site-deploy" -f lanyard-deploy -N ""` (never commit it; `.gitignore` already skips `lanyard-deploy*`). To rotate it later, make a new pair, add the new public key in CloudPanel, replace the `SITE_SSH_KEY` secret, then delete the old public key. In CloudPanel open the
   site, SSH/FTP, and add `lanyard-deploy.pub` under SSH keys for the site user (or append it to
   `~/.ssh/authorized_keys` of that user). This key can only reach that one site user.
4. **Repository secrets** (Settings, Secrets and variables, Actions):

   | secret | value |
   | --- | --- |
   | `SITE_SSH_HOST` | the server's name or IP |
   | `SITE_SSH_USER` | the site user CloudPanel created |
   | `SITE_SSH_KEY` | contents of `lanyard-deploy` (the private key) |
   | `SITE_SSH_KNOWN_HOSTS` | one line, `<host> ssh-ed25519 AAAA...`, the server's own host key (see below), so the job refuses an impostor server |
   | `SITE_SSH_PATH` | `/home/<site user>/htdocs/lanyard.3logiq.com` |

   Optional repository **variables**: `APP_STORE_URL` and `PLAY_STORE_URL`, once the store listings exist (a button stays hidden
   without them).
   **Getting the host key.** The most trustworthy way is to read it on the server (CloudPanel's terminal, or any SSH login):
   `cat /etc/ssh/ssh_host_ed25519_key.pub`, then write the line as `<host> ssh-ed25519 AAAA...` (the same host name or IP you put in
   `SITE_SSH_HOST`; drop the trailing comment). From a Linux machine or WSL, `ssh-keyscan -t ed25519 <host>` prints that line directly.
   The ssh-keyscan that ships with Windows is too old for current Ubuntu servers (`unsupported KEX method sntrup761x25519-sha512@openssh.com`),
   so on Windows use WSL, or the server-side command above.
5. Run the workflow once (Actions, deploy-site, Run workflow). Without the secrets it only builds the zip; with them it
   uploads. The upload deletes files on the server that are no longer in `site/`, and it refuses any path that is not a
   CloudPanel site folder.

## By hand
Download the `lanyard-site` artifact from the workflow run, unzip it, and upload the contents to the site folder in CloudPanel's
file manager.

## What the site expects
- `latest.json` next to `index.html`: the workflow fetches it from the latest GitHub release (`installer/make_latest.py`
  creates it). Without it the download buttons point at the releases page.
- Nothing else: fonts and images are in `site/assets/`, there are no external requests and no analytics.

## Changing the design or icons
Edit `site/`, or the mark in `brand/geometry.mjs` and run `brand/make-icons.mjs` (needs Node 22, `playwright`, Chromium and
ImageMagick; it rewrites every app icon and the site favicons from the one definition).
