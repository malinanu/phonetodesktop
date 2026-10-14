#!/usr/bin/env python3
"""Write latest.json and SHA256SUMS for a release.

    make_latest.py ASSET_DIR VERSION REPO [--tag vX.Y.Z] [--base-url URL] [--page-url URL]

Looks at the files in ASSET_DIR, recognises each by its name, and writes ASSET_DIR/latest.json (read by the
website) and ASSET_DIR/SHA256SUMS. Files it does not recognise are still listed in SHA256SUMS.

By default the download links point at the GitHub release. With --base-url (for example
https://lanyard.3logiq.com/downloads/1.2.3) they point at the website's own copy of the files instead, the GitHub
link is kept as "mirror", and --page-url names the page that lists them.
"""
import hashlib
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

# (regex on the file name, platform, arch, kind)
PATTERNS = [
    (r"^Lanyard-Setup-.+\.exe$", "windows", "x86_64", "installer"),
    (r"^Lanyard-.+-macos\.dmg$", "macos", "universal", "installer"),
    (r"^phone-remote_.+_amd64\.deb$", "linux", "x86_64", "deb"),
    (r"^phone-remote_.+_arm64\.deb$", "linux", "aarch64", "deb"),
    (r"^phone-remote-.+-linux-x86_64\.tar\.gz$", "linux", "x86_64", "tarball"),
    (r"^phone-remote-.+-linux-aarch64\.tar\.gz$", "linux", "aarch64", "tarball"),
    (r"^Lanyard-[0-9][^/]*\.apk$", "android", "universal", "apk"),
]


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def classify(name: str):
    for rx, platform, arch, kind in PATTERNS:
        if re.match(rx, name):
            return platform, arch, kind
    return None


def build(asset_dir: Path, version: str, repo: str, tag: str, base_url: str = "", page_url: str = ""):
    files = sorted(p for p in asset_dir.iterdir() if p.is_file() and p.name not in ("latest.json", "SHA256SUMS"))
    assets, sums = [], []
    for p in files:
        digest = sha256(p)
        sums.append(f"{digest}  {p.name}")
        found = classify(p.name)
        if found:
            platform, arch, kind = found
            github = f"https://github.com/{repo}/releases/download/{tag}/{p.name}"
            asset = {
                "platform": platform,
                "arch": arch,
                "kind": kind,
                "name": p.name,
                "url": f"{base_url.rstrip('/')}/{p.name}" if base_url else github,
                "sha256": digest,
                "size": p.stat().st_size,
            }
            if base_url:
                asset["mirror"] = github
            assets.append(asset)
    manifest = {
        "version": version,
        "tag": tag,
        "released": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "release_page": page_url or f"https://github.com/{repo}/releases/tag/{tag}",
        "github_release_page": f"https://github.com/{repo}/releases/tag/{tag}",
        "assets": assets,
    }
    if base_url:
        manifest["checksums"] = f"{base_url.rstrip('/')}/SHA256SUMS"
    return manifest, "\n".join(sums) + "\n"


def main(argv):
    def option(name):
        return next((argv[i + 1] for i, a in enumerate(argv) if a == name and i + 1 < len(argv)), None)

    tag, base_url, page_url = option("--tag"), option("--base-url") or "", option("--page-url") or ""
    values = {tag, base_url, page_url}
    args = [a for a in argv if not a.startswith("--") and a not in values]
    if len(args) < 3:
        print(__doc__)
        return 2
    asset_dir, version, repo = Path(args[0]), args[1], args[2]
    tag = tag or f"v{version}"
    manifest, sums = build(asset_dir, version, repo, tag, base_url, page_url)
    (asset_dir / "latest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (asset_dir / "SHA256SUMS").write_text(sums)
    print(f"latest.json: {len(manifest['assets'])} recognised file(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
