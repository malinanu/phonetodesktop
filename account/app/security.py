"""Random tokens, sign-in codes and access tokens."""
from __future__ import annotations

import base64
import hashlib
import hmac
import secrets
import time

import jwt

# No 0/O/1/I/L: easier to read out and type.
_CODE_ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789"


def b64u(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def b64u_decode(s: str) -> bytes:
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


def new_id() -> str:
    return secrets.token_hex(16)


def new_token() -> str:
    """An opaque secret (refresh token): 256 bits, base64url."""
    return b64u(secrets.token_bytes(32))


def hash_token(token: str) -> str:
    """Only hashes are stored, so a database leak does not leak usable tokens."""
    return hashlib.sha256(token.encode()).hexdigest()


def new_code() -> str:
    """A short human code like ``K7QM-X2PD`` (40 bits; always paired with an attempt limit and a short life)."""
    raw = "".join(secrets.choice(_CODE_ALPHABET) for _ in range(8))
    return f"{raw[:4]}-{raw[4:]}"


def normalize_code(code: str) -> str:
    return "".join(c for c in code.upper() if c.isalnum())


def equal(a: str, b: str) -> bool:
    return hmac.compare_digest(a.encode(), b.encode())


def make_access_token(user_id: str, secret: str, minutes: int, now: int | None = None) -> str:
    now = int(time.time()) if now is None else now
    return jwt.encode({"sub": user_id, "iat": now, "exp": now + minutes * 60, "typ": "access"}, secret, algorithm="HS256")


def read_access_token(token: str, secret: str, now: int | None = None) -> str | None:
    """The user id, or None for anything invalid or expired. Expiry is checked against ``now`` (the service clock)."""
    now = int(time.time()) if now is None else now
    try:
        claims = jwt.decode(token, secret, algorithms=["HS256"], options={"require": ["exp", "sub"], "verify_exp": False, "verify_iat": False})
    except jwt.PyJWTError:
        return None
    if claims.get("typ") != "access" or not isinstance(claims.get("exp"), int) or claims["exp"] <= now:
        return None
    return claims["sub"]
