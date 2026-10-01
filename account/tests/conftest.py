import time

import jwt
import pytest
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
from fastapi.testclient import TestClient

from app import certs
from app.config import Settings
from app.mail import MemoryMailer
from app.main import create_app
from app.oidc import OidcVerifier
from app.security import b64u

GOOGLE_AUD = "google-client-id.apps.googleusercontent.com"
APPLE_AUD = "app.phoneremote"


class Clock:
    """The service's clock, so tests can move time."""

    def __init__(self) -> None:
        self.t = int(time.time())

    def __call__(self) -> float:
        return self.t

    def advance(self, seconds: int) -> None:
        self.t += seconds


class World:
    def __init__(self, tmp_path) -> None:
        self.clock = Clock()
        self.mailer = MemoryMailer()
        self.rsa = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        self.settings = Settings(
            database_url="sqlite://",
            signing_keys={"k1": bytes([1]) * 32, "k0": bytes([2]) * 32},
            jwt_secret="x" * 40,
            google_client_ids=[GOOGLE_AUD],
            apple_client_ids=[APPLE_AUD],
        )
        oidc = OidcVerifier([GOOGLE_AUD], [APPLE_AUD], key_resolver=lambda provider, token: self.rsa.public_key())
        self.app = create_app(self.settings, mailer=self.mailer, oidc=oidc, clock=self.clock)
        self.client = TestClient(self.app)
        self.public_keys = {k: certs.public_key_bytes(s) for k, s in self.settings.signing_keys.items()}

    # --- helpers -------------------------------------------------------------------------------------
    def sign_in(self, email="me@mail.dev") -> dict:
        assert self.client.post("/v1/auth/email", json={"email": email}).status_code == 202
        code = [c for e, c in self.mailer.sent if e == email.lower()][-1]
        r = self.client.post("/v1/auth/verify", json={"email": email, "code": code})
        assert r.status_code == 200, r.text
        return r.json()

    def auth(self, tokens) -> dict:
        return {"Authorization": f"Bearer {tokens['access_token']}"}

    def new_device(self, name="Pixel", kind="phone", platform="android", device_id=None) -> dict:
        sk = Ed25519PrivateKey.generate()
        pk = b64u(sk.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw))
        return {"id": device_id or b64u(sk.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw))[:16], "name": name, "platform": platform, "kind": kind, "pubkey": pk, "_sk": sk}

    @staticmethod
    def body(device: dict) -> dict:
        return {k: v for k, v in device.items() if not k.startswith("_")}

    def id_token(self, provider="google", **overrides) -> str:
        aud = GOOGLE_AUD if provider == "google" else APPLE_AUD
        iss = "https://accounts.google.com" if provider == "google" else "https://appleid.apple.com"
        now = int(time.time())
        claims = {"iss": iss, "aud": aud, "sub": "subject-1", "email": "oidc@mail.dev", "email_verified": True, "iat": now, "exp": now + 600}
        claims.update(overrides)
        claims = {k: v for k, v in claims.items() if v is not None}
        return jwt.encode(claims, self.rsa, algorithm="RS256")


@pytest.fixture
def world(tmp_path):
    return World(tmp_path)
