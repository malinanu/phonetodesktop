import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import make_latest


class MakeLatest(unittest.TestCase):
    def run_in(self, names):
        d = Path(tempfile.mkdtemp())
        for n in names:
            (d / n).write_bytes(n.encode())
        manifest, sums = make_latest.build(d, "1.2.3", "me/repo", "v1.2.3")
        return d, manifest, sums

    def test_every_platform_is_recognised(self):
        names = [
            "PhoneRemote-Setup-1.2.3.exe", "PhoneRemote-1.2.3-macos.dmg",
            "phone-remote_1.2.3-1_amd64.deb", "phone-remote_1.2.3-1_arm64.deb",
            "phone-remote-1.2.3-linux-x86_64.tar.gz", "phone-remote-1.2.3-linux-aarch64.tar.gz",
            "PhoneRemote-1.2.3.apk",
        ]
        _, m, _ = self.run_in(names)
        got = {(a["platform"], a["arch"], a["kind"]) for a in m["assets"]}
        self.assertEqual(got, {
            ("windows", "x86_64", "installer"), ("macos", "universal", "installer"),
            ("linux", "x86_64", "deb"), ("linux", "aarch64", "deb"),
            ("linux", "x86_64", "tarball"), ("linux", "aarch64", "tarball"),
            ("android", "universal", "apk"),
        })
        self.assertEqual(m["version"], "1.2.3")
        self.assertEqual(m["release_page"], "https://github.com/me/repo/releases/tag/v1.2.3")

    def test_urls_and_checksums(self):
        d, m, sums = self.run_in(["PhoneRemote-1.2.3.apk"])
        a = m["assets"][0]
        self.assertEqual(a["url"], "https://github.com/me/repo/releases/download/v1.2.3/PhoneRemote-1.2.3.apk")
        self.assertEqual(a["sha256"], hashlib.sha256(b"PhoneRemote-1.2.3.apk").hexdigest())
        self.assertEqual(a["size"], len(b"PhoneRemote-1.2.3.apk"))
        self.assertIn(f"{a['sha256']}  PhoneRemote-1.2.3.apk", sums)

    def test_unknown_files_are_only_in_the_checksums(self):
        _, m, sums = self.run_in(["notes.txt", "PhoneRemote-Setup-1.2.3.exe"])
        self.assertEqual([a["name"] for a in m["assets"]], ["PhoneRemote-Setup-1.2.3.exe"])
        self.assertIn("notes.txt", sums)

    def test_generated_files_are_not_hashed_again(self):
        d, _, _ = self.run_in(["PhoneRemote-1.2.3.apk"])
        make_latest.main([str(d), "1.2.3", "me/repo"])
        make_latest.main([str(d), "1.2.3", "me/repo"])  # second run must not list latest.json / SHA256SUMS
        self.assertNotIn("latest.json", (d / "SHA256SUMS").read_text())
        self.assertEqual(json.loads((d / "latest.json").read_text())["tag"], "v1.2.3")

    def test_a_pre_release_tag_is_kept(self):
        d = Path(tempfile.mkdtemp())
        (d / "PhoneRemote-1.2.3-rc1.apk").write_bytes(b"x")
        make_latest.main([str(d), "1.2.3-rc1", "me/repo", "--tag", "v1.2.3-rc1"])
        a = json.loads((d / "latest.json").read_text())["assets"][0]
        self.assertTrue(a["url"].startswith("https://github.com/me/repo/releases/download/v1.2.3-rc1/"))


if __name__ == "__main__":
    unittest.main()
