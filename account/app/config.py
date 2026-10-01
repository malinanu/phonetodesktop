"""Settings, read from the environment (see docs/ACCOUNTS.md)."""
from __future__ import annotations

import base64
import json
import os
import secrets
from dataclasses import dataclass, field


def _b64u_decode(s: str) -> bytes:
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


@dataclass
class Settings:
    database_url: str = "sqlite:///./account.db"
    # kid -> 32-byte Ed25519 seed. The FIRST key signs new certificates; the rest only exist so
    # certificates signed before a rotation stay valid until they expire.
    signing_keys: dict[str, bytes] = field(default_factory=dict)
    jwt_secret: str = ""
    # In development the sign-in code is written to the log instead of being emailed.
    dev_mode: bool = False
    smtp_host: str = ""
    smtp_port: int = 587
    smtp_user: str = ""
    smtp_password: str = ""
    smtp_from: str = "Phone Remote <no-reply@localhost>"
    smtp_starttls: bool = True
    google_client_ids: list[str] = field(default_factory=list)
    apple_client_ids: list[str] = field(default_factory=list)
    cert_days: int = 30
    access_minutes: int = 15
    refresh_days: int = 60
    code_minutes: int = 15
    code_attempts: int = 5
    link_minutes: int = 10
    # Behind a reverse proxy the real client address is in X-Forwarded-For; only trust it when asked to.
    trust_proxy: bool = False

    @classmethod
    def from_env(cls) -> Settings:
        env = os.environ.get
        keys: dict[str, bytes] = {}
        raw = env("ACCOUNT_SIGNING_KEYS", "")
        path = env("ACCOUNT_SIGNING_KEYS_FILE")
        if path:
            with open(path, encoding="utf-8") as f:
                raw = f.read().strip()
        # "k1:<base64url seed>,k0:<base64url seed>" or the same as a JSON object
        if raw.startswith("{"):
            keys = {k: _b64u_decode(v) for k, v in json.loads(raw).items()}
        else:
            for part in filter(None, (p.strip() for p in raw.split(","))):
                kid, _, seed = part.partition(":")
                keys[kid] = _b64u_decode(seed)
        dev = env("ACCOUNT_DEV_MODE", "") == "1"
        secret = env("ACCOUNT_JWT_SECRET", "")
        if not dev and (not keys or len(secret) < 32):
            raise RuntimeError("Set ACCOUNT_SIGNING_KEYS (or _FILE) and ACCOUNT_JWT_SECRET (32+ characters), or ACCOUNT_DEV_MODE=1 for local use.")
        if dev:
            if not keys:
                keys = {"dev": secrets.token_bytes(32)}
            secret = secret or secrets.token_urlsafe(48)
        for kid, seed in keys.items():
            if len(seed) != 32:
                raise RuntimeError(f"signing key {kid!r} must be a 32-byte seed")
        csv = lambda name: [x.strip() for x in env(name, "").split(",") if x.strip()]  # noqa: E731
        return cls(
            database_url=env("DATABASE_URL", "sqlite:///./account.db"),
            signing_keys=keys,
            jwt_secret=secret,
            dev_mode=dev,
            smtp_host=env("SMTP_HOST", ""),
            smtp_port=int(env("SMTP_PORT", "587")),
            smtp_user=env("SMTP_USER", ""),
            smtp_password=env("SMTP_PASSWORD", ""),
            smtp_from=env("SMTP_FROM", "Phone Remote <no-reply@localhost>"),
            smtp_starttls=env("SMTP_STARTTLS", "1") != "0",
            google_client_ids=csv("GOOGLE_CLIENT_IDS"),
            apple_client_ids=csv("APPLE_CLIENT_IDS"),
            cert_days=int(env("ACCOUNT_CERT_DAYS", "30")),
            trust_proxy=env("ACCOUNT_TRUST_PROXY", "") == "1",
        )
