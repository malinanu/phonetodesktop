"""Device certificates: how a PC learns, with no internet, that a phone belongs to the same account.

A certificate is ``b64u(payload) + "." + b64u(signature)`` where the signature (Ed25519, by the account service's
key) covers ``b"PRv2-cert\\0" + payload_bytes``. The verifier checks the exact bytes it received, so no JSON
canonicalisation has to match between implementations. Payload fields:

    v     1
    kid   which service key signed it
    acct  account id
    dev   device id (the same id the device uses on the local protocol)
    kind  "phone" or "pc"
    name  device name shown to the owner
    pk    the device's Ed25519 public key (base64url), the one it signs local logins with
    iat / exp   unix seconds
"""
from __future__ import annotations

import json
from typing import Any

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from .security import b64u, b64u_decode

CERT_PREFIX = b"PRv2-cert\x00"
ENVELOPE_PREFIX = b"PRv2-signed\x00"


class CertError(Exception):
    """The certificate is not valid. The message says why (it is safe to show)."""


def public_key_bytes(seed: bytes) -> bytes:
    return Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)


def _payload_bytes(payload: dict[str, Any]) -> bytes:
    return json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def issue(seed: bytes, kid: str, payload: dict[str, Any]) -> str:
    body = _payload_bytes({**payload, "v": 1, "kid": kid})
    sig = Ed25519PrivateKey.from_private_bytes(seed).sign(CERT_PREFIX + body)
    return f"{b64u(body)}.{b64u(sig)}"


def verify(cert: str, public_keys: dict[str, bytes], now: int, *, skew: int = 300, grace: int = 0) -> dict[str, Any]:
    """The payload of a valid certificate. ``grace`` extends the life after ``exp`` (for offline devices)."""
    try:
        body_b64, sig_b64 = cert.split(".")
        body, sig = b64u_decode(body_b64), b64u_decode(sig_b64)
        payload = json.loads(body)
    except Exception as e:  # malformed text, base64 or JSON
        raise CertError("malformed certificate") from e
    if not isinstance(payload, dict) or payload.get("v") != 1:
        raise CertError("unsupported certificate version")
    key = public_keys.get(payload.get("kid"))
    if key is None:
        raise CertError("signed by an unknown key")
    try:
        Ed25519PublicKey.from_public_bytes(key).verify(sig, CERT_PREFIX + body)
    except (InvalidSignature, ValueError) as e:
        raise CertError("bad signature") from e
    iat, exp = payload.get("iat"), payload.get("exp")
    if not isinstance(iat, int) or not isinstance(exp, int):
        raise CertError("missing dates")
    if iat > now + skew:
        raise CertError("not valid yet")
    if exp + grace < now:
        raise CertError("expired")
    for field in ("acct", "dev", "kind", "pk"):
        if not isinstance(payload.get(field), str) or not payload[field]:
            raise CertError(f"missing {field}")
    return payload


def sign_envelope(seed: bytes, kid: str, payload: dict[str, Any]) -> dict[str, Any]:
    """A signed JSON document (used for the revocation list)."""
    body = _payload_bytes(payload)
    sig = Ed25519PrivateKey.from_private_bytes(seed).sign(ENVELOPE_PREFIX + body)
    return {"payload": b64u(body), "sig": b64u(sig), "kid": kid}


def verify_envelope(envelope: dict[str, Any], public_keys: dict[str, bytes]) -> dict[str, Any]:
    try:
        body, sig = b64u_decode(envelope["payload"]), b64u_decode(envelope["sig"])
        key = public_keys[envelope["kid"]]
        Ed25519PublicKey.from_public_bytes(key).verify(sig, ENVELOPE_PREFIX + body)
        return json.loads(body)
    except Exception as e:
        raise CertError("bad signature") from e
