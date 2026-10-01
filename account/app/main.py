"""Phone Remote account service.

Phone Remote works without it. An account only adds: a list of your devices, and short-lived signed certificates
that let a PC accept your phone with no QR code (and later, with no internet). Nothing about media control or
files passes through here.
"""

import re
import time
from typing import Annotated, Literal

from fastapi import Depends, FastAPI, Header, HTTPException, Request, Response
from pydantic import BaseModel, EmailStr, Field, field_validator
from sqlalchemy import select
from sqlalchemy.orm import Session as DbSession

from . import certs
from .config import Settings
from .db import Device, EmailCode, Identity, LinkCode, Session, User, make_engine, make_sessionmaker
from .mail import LogMailer, Mailer, SmtpMailer
from .oidc import OidcError, OidcVerifier
from .ratelimit import RateLimiter
from .security import (
    b64u,
    b64u_decode,
    equal,
    hash_token,
    make_access_token,
    new_code,
    new_token,
    normalize_code,
    read_access_token,
)

_DEVICE_ID = re.compile(r"^[A-Za-z0-9_-]{8,64}$")
_PLATFORM = re.compile(r"^[a-z0-9_-]{1,16}$")


class EmailIn(BaseModel):
    email: EmailStr


class VerifyIn(BaseModel):
    email: EmailStr
    code: str = Field(min_length=6, max_length=16)


class OidcIn(BaseModel):
    provider: Literal["google", "apple"]
    id_token: str = Field(min_length=20, max_length=8192)
    nonce: str | None = Field(default=None, max_length=256)


class RefreshIn(BaseModel):
    refresh_token: str = Field(min_length=20, max_length=200)


class DeviceIn(BaseModel):
    id: str
    name: str = Field(min_length=1, max_length=80)
    platform: str
    kind: Literal["phone", "pc"]
    pubkey: str

    @field_validator("id")
    @classmethod
    def _id(cls, v: str) -> str:
        if not _DEVICE_ID.match(v):
            raise ValueError("invalid device id")
        return v

    @field_validator("platform")
    @classmethod
    def _platform(cls, v: str) -> str:
        if not _PLATFORM.match(v):
            raise ValueError("invalid platform")
        return v

    @field_validator("name")
    @classmethod
    def _name(cls, v: str) -> str:
        v = "".join(c for c in v if c.isprintable()).strip()
        if not v:
            raise ValueError("invalid name")
        return v

    @field_validator("pubkey")
    @classmethod
    def _pubkey(cls, v: str) -> str:
        try:
            raw = b64u_decode(v)
            from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

            Ed25519PublicKey.from_public_bytes(raw)
        except Exception as e:
            raise ValueError("invalid public key") from e
        if b64u(raw) != v:
            raise ValueError("invalid public key")
        return v


class ClaimIn(DeviceIn):
    code: str = Field(min_length=6, max_length=16)


class RenameIn(BaseModel):
    name: str = Field(min_length=1, max_length=80)


def create_app(settings: Settings | None = None, *, mailer: Mailer | None = None, oidc: OidcVerifier | None = None, clock=time.time) -> FastAPI:
    settings = settings or Settings.from_env()
    if mailer is None:
        mailer = SmtpMailer(settings.smtp_host, settings.smtp_port, settings.smtp_user, settings.smtp_password, settings.smtp_from, settings.smtp_starttls) if settings.smtp_host else LogMailer()
    oidc = oidc or OidcVerifier(settings.google_client_ids, settings.apple_client_ids)
    engine = make_engine(settings.database_url)
    sessions = make_sessionmaker(engine)

    kid, seed = next(iter(settings.signing_keys.items()))
    public_keys = {k: certs.public_key_bytes(s) for k, s in settings.signing_keys.items()}

    limit_email_ip = RateLimiter(20, 3600)
    limit_email_addr = RateLimiter(5, 3600)
    limit_verify_ip = RateLimiter(30, 600)
    limit_claim_ip = RateLimiter(10, 600)
    limit_public_ip = RateLimiter(120, 60)

    app = FastAPI(title="Phone Remote accounts", version="1.0.0", docs_url=None, redoc_url=None)
    app.state.settings, app.state.engine = settings, engine

    def now() -> int:
        return int(clock())

    def db_dep():
        db = sessions()
        try:
            yield db
        finally:
            db.close()

    Db = Annotated[DbSession, Depends(db_dep)]

    def client_ip(request: Request) -> str:
        if settings.trust_proxy:
            fwd = request.headers.get("x-forwarded-for", "")
            if fwd:
                return fwd.split(",")[-1].strip()  # the address our own proxy appended
        return request.client.host if request.client else "unknown"

    def current_user(db: Db, authorization: Annotated[str | None, Header()] = None) -> User:
        if not authorization or not authorization.lower().startswith("bearer "):
            raise HTTPException(401, "Sign in first.", headers={"WWW-Authenticate": "Bearer"})
        user_id = read_access_token(authorization[7:].strip(), settings.jwt_secret, now())
        user = db.get(User, user_id) if user_id else None
        if user is None:
            raise HTTPException(401, "Your session expired. Sign in again.", headers={"WWW-Authenticate": "Bearer"})
        return user

    CurrentUser = Annotated[User, Depends(current_user)]

    def issue_tokens(db: DbSession, user: User) -> dict:
        refresh = new_token()
        db.add(Session(user_id=user.id, refresh_hash=hash_token(refresh), expires_at=now() + settings.refresh_days * 86400, created_at=now()))
        db.commit()
        return {
            "access_token": make_access_token(user.id, settings.jwt_secret, settings.access_minutes, now()),
            "refresh_token": refresh,
            "expires_in": settings.access_minutes * 60,
            "user": {"id": user.id, "email": user.email},
        }

    def device_json(d: Device) -> dict:
        return {"id": d.id, "name": d.name, "platform": d.platform, "kind": d.kind, "created_at": d.created_at, "revoked": d.revoked_at is not None}

    def make_cert(db: DbSession, d: Device) -> str:
        t = now()
        cert = certs.issue(seed, kid, {"acct": d.user_id, "dev": d.id, "kind": d.kind, "name": d.name, "pk": d.pubkey, "iat": t, "exp": t + settings.cert_days * 86400})
        d.cert_issued_at = t
        db.commit()
        return cert

    def keys_doc() -> dict:
        return {"v": 1, "current": kid, "keys": [{"kid": k, "alg": "Ed25519", "pk": b64u(pk)} for k, pk in public_keys.items()]}

    def enroll(db: DbSession, user: User, body: DeviceIn) -> Device:
        existing = db.get(Device, body.id)
        if existing is not None:
            if existing.user_id != user.id or existing.pubkey != body.pubkey:
                raise HTTPException(409, "That device id is already in use.")
            if existing.revoked_at is not None:
                raise HTTPException(403, "This device was removed from the account. Create a new device identity to add it again.")
            existing.name, existing.platform, existing.kind = body.name, body.platform, body.kind
            db.commit()
            return existing
        if db.scalar(select(Device).where(Device.user_id == user.id, Device.pubkey == body.pubkey, Device.revoked_at.is_(None))):
            raise HTTPException(409, "That key is already enrolled under another id.")
        d = Device(id=body.id, user_id=user.id, name=body.name, platform=body.platform, kind=body.kind, pubkey=body.pubkey, created_at=now())
        db.add(d)
        db.commit()
        return d

    # ---- public ----------------------------------------------------------------------------------------

    @app.get("/health")
    def health():
        return {"ok": True}

    @app.get("/.well-known/phoneremote.json")
    def well_known(request: Request):
        limit_public_ip.check(client_ip(request))
        return keys_doc()

    @app.get("/v1/accounts/{account_id}/revocations")
    def revocations(account_id: str, request: Request, db: Db, since: int = 0):
        """Which of this account's devices have been removed. Signed, so a PC can trust it from any network path.
        The account id is an unguessable random id known only to the account's own devices."""
        limit_public_ip.check(client_ip(request))
        if not re.fullmatch(r"[0-9a-f]{32}", account_id):
            raise HTTPException(404, "Not found")
        rows = db.scalars(select(Device).where(Device.user_id == account_id, Device.revoked_at.is_not(None), Device.revoked_at >= since)).all()
        return certs.sign_envelope(seed, kid, {"acct": account_id, "now": now(), "revoked": [{"dev": d.id, "at": d.revoked_at} for d in rows]})

    # ---- signing in ------------------------------------------------------------------------------------

    @app.post("/v1/auth/email", status_code=202)
    def request_code(body: EmailIn, request: Request, db: Db):
        email = body.email.lower()
        limit_email_ip.check(client_ip(request))
        limit_email_addr.check(email)
        code = new_code()
        db.merge(EmailCode(email=email, code_hash=hash_token(normalize_code(code)), expires_at=now() + settings.code_minutes * 60, attempts=0))
        db.commit()
        try:
            mailer.send_code(email, code, settings.code_minutes)
        except Exception:
            raise HTTPException(502, "Could not send the email. Try again later.") from None
        return {"ok": True}  # the same answer whether or not the address has an account

    @app.post("/v1/auth/verify")
    def verify_code(body: VerifyIn, request: Request, db: Db):
        limit_verify_ip.check(client_ip(request))
        email = body.email.lower()
        row = db.get(EmailCode, email)
        bad = HTTPException(401, "That code is not valid. Ask for a new one.")
        if row is None:
            raise bad
        if row.expires_at < now() or row.attempts >= settings.code_attempts:
            db.delete(row)
            db.commit()
            raise bad
        if not equal(row.code_hash, hash_token(normalize_code(body.code))):
            row.attempts += 1
            if row.attempts >= settings.code_attempts:
                db.delete(row)  # too many wrong guesses: the attacker must request (and be rate-limited for) a new code
            db.commit()
            raise bad
        db.delete(row)
        user = db.scalar(select(User).where(User.email == email))
        if user is None:
            user = User(email=email, created_at=now())
            db.add(user)
            db.commit()
        return issue_tokens(db, user)

    @app.post("/v1/auth/oidc")
    def sign_in_with_provider(body: OidcIn, request: Request, db: Db):
        limit_verify_ip.check(client_ip(request))
        try:
            subject, email = oidc.verify(body.provider, body.id_token, body.nonce)
        except OidcError as e:
            raise HTTPException(401, str(e)) from None
        ident = db.scalar(select(Identity).where(Identity.provider == body.provider, Identity.subject == subject))
        if ident is not None:
            user = db.get(User, ident.user_id)
        else:
            # The provider vouches for this address, so it joins the account that already uses it.
            user = db.scalar(select(User).where(User.email == email))
            if user is None:
                user = User(email=email, created_at=now())
                db.add(user)
                db.flush()
            db.add(Identity(user_id=user.id, provider=body.provider, subject=subject))
            db.commit()
        return issue_tokens(db, user)

    @app.post("/v1/auth/refresh")
    def refresh(body: RefreshIn, db: Db):
        h = hash_token(body.refresh_token)
        s = db.scalar(select(Session).where(Session.refresh_hash == h))
        if s is None:
            reused = db.scalar(select(Session).where(Session.previous_hash == h))
            if reused is not None:  # an already-used token came back: it was copied. End that session.
                reused.revoked = True
                db.commit()
            raise HTTPException(401, "Sign in again.")
        if s.revoked or s.expires_at < now():
            raise HTTPException(401, "Sign in again.")
        user = db.get(User, s.user_id)
        new_refresh = new_token()
        s.previous_hash, s.refresh_hash = h, hash_token(new_refresh)
        db.commit()
        return {
            "access_token": make_access_token(user.id, settings.jwt_secret, settings.access_minutes, now()),
            "refresh_token": new_refresh,
            "expires_in": settings.access_minutes * 60,
            "user": {"id": user.id, "email": user.email},
        }

    @app.post("/v1/auth/logout", status_code=204)
    def logout(body: RefreshIn, db: Db):
        s = db.scalar(select(Session).where(Session.refresh_hash == hash_token(body.refresh_token)))
        if s is not None:
            s.revoked = True
            db.commit()
        return Response(status_code=204)

    # ---- the account -----------------------------------------------------------------------------------

    @app.get("/v1/me")
    def me(user: CurrentUser):
        return {"id": user.id, "email": user.email, "created_at": user.created_at}

    @app.delete("/v1/me", status_code=204)
    def delete_account(user: CurrentUser, db: Db):
        for link in db.scalars(select(LinkCode).where(LinkCode.user_id == user.id)):
            db.delete(link)
        db.delete(user)
        db.commit()
        return Response(status_code=204)

    # ---- devices ---------------------------------------------------------------------------------------

    @app.post("/v1/devices", status_code=201)
    def add_device(body: DeviceIn, user: CurrentUser, db: Db):
        d = enroll(db, user, body)
        return {"device": device_json(d), "cert": make_cert(db, d), "account_id": user.id, "keys": keys_doc()}

    @app.get("/v1/devices")
    def list_devices(user: CurrentUser, db: Db):
        rows = db.scalars(select(Device).where(Device.user_id == user.id).order_by(Device.created_at)).all()
        return {"devices": [device_json(d) for d in rows]}

    def own_device(db: DbSession, user: User, device_id: str) -> Device:
        d = db.get(Device, device_id)
        if d is None or d.user_id != user.id:
            raise HTTPException(404, "No such device.")
        return d

    @app.patch("/v1/devices/{device_id}")
    def rename_device(device_id: str, body: RenameIn, user: CurrentUser, db: Db):
        d = own_device(db, user, device_id)
        name = "".join(c for c in body.name if c.isprintable()).strip()
        if not name:
            raise HTTPException(422, "Give the device a name.")
        d.name = name
        db.commit()
        return device_json(d)

    @app.delete("/v1/devices/{device_id}", status_code=204)
    def remove_device(device_id: str, user: CurrentUser, db: Db):
        d = own_device(db, user, device_id)
        if d.revoked_at is None:
            d.revoked_at = now()  # kept (not deleted) so the revocation list can still name it
            db.commit()
        return Response(status_code=204)

    @app.post("/v1/devices/{device_id}/cert")
    def renew_cert(device_id: str, user: CurrentUser, db: Db):
        d = own_device(db, user, device_id)
        if d.revoked_at is not None:
            raise HTTPException(403, "This device was removed from the account.")
        return {"cert": make_cert(db, d), "keys": keys_doc()}

    # ---- joining a device that has no login screen (a PC) ------------------------------------------------

    @app.post("/v1/links", status_code=201)
    def create_link(user: CurrentUser, db: Db):
        code = new_code()
        db.add(LinkCode(code_hash=hash_token(normalize_code(code)), user_id=user.id, expires_at=now() + settings.link_minutes * 60))
        db.commit()
        return {"code": code, "expires_in": settings.link_minutes * 60}

    @app.post("/v1/links/claim", status_code=201)
    def claim_link(body: ClaimIn, request: Request, db: Db):
        limit_claim_ip.check(client_ip(request))
        link = db.get(LinkCode, hash_token(normalize_code(body.code)))
        if link is None or link.expires_at < now():
            if link is not None:
                db.delete(link)
                db.commit()
            raise HTTPException(401, "That code is not valid or has expired.")
        user = db.get(User, link.user_id)
        db.delete(link)  # single use, even if enrolling below fails
        db.commit()
        d = enroll(db, user, body)
        return {"device": device_json(d), "cert": make_cert(db, d), "account_id": user.id, "keys": keys_doc()}

    return app
