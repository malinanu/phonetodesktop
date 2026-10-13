"""Verifying Google and Apple ID tokens (Sign in with Apple is required on iOS when other social sign-in is offered)."""
from __future__ import annotations

from collections.abc import Callable

import jwt

_PROVIDERS = {
    "google": {"issuers": ("https://accounts.google.com", "accounts.google.com"), "jwks": "https://www.googleapis.com/oauth2/v3/certs"},
    "apple": {"issuers": ("https://appleid.apple.com",), "jwks": "https://appleid.apple.com/auth/keys"},
}


class OidcError(Exception):
    pass


class OidcVerifier:
    """``key_resolver(provider, id_token)`` returns the public key that signed the token (tests inject their own)."""

    def __init__(self, google_ids: list[str], apple_ids: list[str], key_resolver: Callable[[str, str], object] | None = None) -> None:
        self.audiences = {"google": google_ids, "apple": apple_ids}
        self._clients: dict[str, jwt.PyJWKClient] = {}
        self.key_resolver = key_resolver or self._fetch_key

    def _fetch_key(self, provider: str, token: str):
        client = self._clients.get(provider)
        if client is None:
            client = self._clients[provider] = jwt.PyJWKClient(_PROVIDERS[provider]["jwks"], cache_keys=True, lifespan=3600, timeout=8)
        return client.get_signing_key_from_jwt(token).key

    def enabled(self, provider: str) -> bool:
        return provider in _PROVIDERS and bool(self.audiences.get(provider))

    def verify(self, provider: str, id_token: str, nonce: str | None = None) -> tuple[str, str]:
        """(subject, email) of a valid token whose email is verified."""
        if not self.enabled(provider):
            raise OidcError("this sign-in method is not set up")
        spec = _PROVIDERS[provider]
        try:
            key = self.key_resolver(provider, id_token)
            claims = jwt.decode(
                id_token,
                key,  # type: ignore[arg-type]
                algorithms=["RS256", "ES256"],
                audience=self.audiences[provider],
                options={"require": ["exp", "iat", "iss", "aud", "sub"]},
                leeway=30,
            )
        except jwt.PyJWTError as e:
            raise OidcError("invalid sign-in token") from e
        if claims["iss"] not in spec["issuers"]:
            raise OidcError("invalid sign-in token")
        if nonce is not None and claims.get("nonce") != nonce:
            raise OidcError("invalid sign-in token")
        email = claims.get("email")
        verified = claims.get("email_verified")
        if not email or verified not in (True, "true"):
            raise OidcError("the provider has not verified an email address for this account")
        return str(claims["sub"]), str(email).lower()
