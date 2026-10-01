#!/usr/bin/env python3
"""Write latest.json and SHA256SUMS for a release.

    make_latest.py ASSET_DIR VERSION REPO [--tag vX.Y.Z]

Looks at the files in ASSET_DIR, recognises each by its name, and writes ASSET_DIR/latest.json (read by the
website) and ASSET_DIR/SHA256SUMS. Files it does not recognise are still listed in SHA256SUMS.
"""
import hashlib
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

# (regex on the file name, platform, arch, kind)
PATTERNS = [
    (r"^PhoneRemote-Setup-.+\.exe$", "windows", "x86_64", "installer"),
    (r"^PhoneRemote-.+-macos\.dmg$", "macos", "universal", "installer"),
    (r"^phone-remote_.+_amd64\.deb$", "linux", "x86_64", "deb"),
    (r"^phone-remote_.+_arm64\.deb$", "linux", "aarch64", "deb"),
    (r"^phone-remote-.+-linux-x86_64\.tar\.gz$", "linux", "x86_64", "tarball"),
    (r"^phone-remote-.+-linux-aarch64\.tar\.gz$", "linux", "aarch64", "tarball"),
    (r"^PhoneRemote-[0-9][^/]*\.apk$", "android", "universal", "apk"),
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


def build(asset_dir: Path, version: str, repo: str, tag: str):
    files = sorted(p for p in asset_dir.iterdir() if p.is_file() and p.name not in ("latest.json", "SHA256SUMS"))
    assets, sums = [], []
    for p in files:
        digest = sha256(p)
        sums.append(f"{digest}  {p.name}")
        found = classify(p.name)
        if found:
            platform, arch, kind = found
            assets.append({
                "platform": platform,
                "arch": arch,
                "kind": kind,
                "name": p.name,
                "url": f"https://github.com/{repo}/releases/download/{tag}/{p.name}",
                "sha256": digest,
                "size": p.stat().st_size,
            })
    manifest = {
        "version": version,
        "tag": tag,
        "released": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "release_page": f"https://github.com/{repo}/releases/tag/{tag}",
        "assets": assets,
    }
    return manifest, "\n".join(sums) + "\n"


def main(argv):
    args = [a for a in argv if not a.startswith("--")]
    tag = next((argv[i + 1] for i, a in enumerate(argv) if a == "--tag" and i + 1 < len(argv)), None)
    if len(args) < 3:
        print(__doc__)
        return 2
    asset_dir, version, repo = Path(args[0]), args[1], args[2]
    tag = tag or f"v{version}"
    manifest, sums = build(asset_dir, version, repo, tag)
    (asset_dir / "latest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (asset_dir / "SHA256SUMS").write_text(sums)
    print(f"latest.json: {len(manifest['assets'])} recognised file(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
