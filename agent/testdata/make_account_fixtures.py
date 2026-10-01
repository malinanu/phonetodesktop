"""Regenerates the account_* files here with the real service code, so the Rust tests check the actual wire format.

Run from the account/ directory:  python3 ../agent/testdata/make_account_fixtures.py
The seeds and times are fixed, so the output is stable; only run it if the certificate format changes.
"""
import json

from app import certs
from app.security import b64u

seed = bytes([1]) * 32
phone_pk = b64u(certs.public_key_bytes(bytes([2]) * 32))
acct = "a" * 32
cert = certs.issue(seed, "k1", {"acct": acct, "dev": "phone-1", "kind": "phone", "name": "Pixel", "pk": phone_pk, "iat": 1_799_999_000, "exp": 1_802_591_000})
env = certs.sign_envelope(seed, "k1", {"acct": acct, "now": 1_800_000_100, "revoked": [{"dev": "phone-1", "at": 1_800_000_050}]})
here = "../agent/testdata/"
open(here + "account_service_pk.txt", "w").write(b64u(certs.public_key_bytes(seed)) + "\n")
open(here + "account_cert.txt", "w").write(cert + "\n")
open(here + "account_revocations.json", "w").write(json.dumps(env, separators=(",", ":")) + "\n")
