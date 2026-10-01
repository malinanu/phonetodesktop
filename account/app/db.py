"""Tables. Only hashes of secrets are stored (codes, refresh tokens, link codes)."""
from __future__ import annotations

import time

from sqlalchemy import BigInteger, ForeignKey, Index, String, create_engine
from sqlalchemy.engine import Engine
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column, relationship, sessionmaker

from .security import new_id


def now() -> int:
    return int(time.time())


class Base(DeclarativeBase):
    pass


class User(Base):
    __tablename__ = "users"
    id: Mapped[str] = mapped_column(String(32), primary_key=True, default=new_id)
    email: Mapped[str] = mapped_column(String(320), unique=True)
    created_at: Mapped[int] = mapped_column(BigInteger, default=now)
    devices: Mapped[list["Device"]] = relationship(cascade="all, delete-orphan", back_populates="user")
    sessions: Mapped[list["Session"]] = relationship(cascade="all, delete-orphan")
    identities: Mapped[list["Identity"]] = relationship(cascade="all, delete-orphan")


class Identity(Base):
    """A Google / Apple sign-in linked to a user."""
    __tablename__ = "identities"
    id: Mapped[str] = mapped_column(String(32), primary_key=True, default=new_id)
    user_id: Mapped[str] = mapped_column(ForeignKey("users.id", ondelete="CASCADE"))
    provider: Mapped[str] = mapped_column(String(16))
    subject: Mapped[str] = mapped_column(String(255))
    __table_args__ = (Index("ix_identity_provider_subject", "provider", "subject", unique=True),)


class EmailCode(Base):
    """A sign-in code sent by email. One live code per address; wrong guesses are counted."""
    __tablename__ = "email_codes"
    email: Mapped[str] = mapped_column(String(320), primary_key=True)
    code_hash: Mapped[str] = mapped_column(String(64))
    expires_at: Mapped[int] = mapped_column(BigInteger)
    attempts: Mapped[int] = mapped_column(default=0)


class Session(Base):
    """A signed-in app. The refresh token rotates on every use; presenting an old one revokes the session."""
    __tablename__ = "sessions"
    id: Mapped[str] = mapped_column(String(32), primary_key=True, default=new_id)
    user_id: Mapped[str] = mapped_column(ForeignKey("users.id", ondelete="CASCADE"), index=True)
    refresh_hash: Mapped[str] = mapped_column(String(64), index=True)
    previous_hash: Mapped[str | None] = mapped_column(String(64), default=None, index=True)
    created_at: Mapped[int] = mapped_column(BigInteger, default=now)
    expires_at: Mapped[int] = mapped_column(BigInteger)
    revoked: Mapped[bool] = mapped_column(default=False)


class Device(Base):
    __tablename__ = "devices"
    id: Mapped[str] = mapped_column(String(64), primary_key=True)  # the id the device uses on the local protocol
    user_id: Mapped[str] = mapped_column(ForeignKey("users.id", ondelete="CASCADE"), index=True)
    name: Mapped[str] = mapped_column(String(80))
    platform: Mapped[str] = mapped_column(String(16))
    kind: Mapped[str] = mapped_column(String(8))  # "phone" | "pc"
    pubkey: Mapped[str] = mapped_column(String(64))  # base64url Ed25519
    created_at: Mapped[int] = mapped_column(BigInteger, default=now)
    cert_issued_at: Mapped[int | None] = mapped_column(BigInteger, default=None)
    revoked_at: Mapped[int | None] = mapped_column(BigInteger, default=None, index=True)
    user: Mapped[User] = relationship(back_populates="devices")


class LinkCode(Base):
    """A one-time code a signed-in user creates so another device (a PC with no login screen) can join the account."""
    __tablename__ = "link_codes"
    code_hash: Mapped[str] = mapped_column(String(64), primary_key=True)
    user_id: Mapped[str] = mapped_column(ForeignKey("users.id", ondelete="CASCADE"), index=True)
    expires_at: Mapped[int] = mapped_column(BigInteger)


def make_engine(url: str) -> Engine:
    if url.startswith("sqlite"):
        from sqlalchemy.pool import StaticPool

        kwargs = {"connect_args": {"check_same_thread": False}}
        if ":memory:" in url or url in ("sqlite://", "sqlite:///"):
            kwargs["poolclass"] = StaticPool
        return create_engine(url, **kwargs)
    if url.startswith("postgresql://"):
        url = url.replace("postgresql://", "postgresql+psycopg://", 1)
    return create_engine(url, pool_pre_ping=True)


def make_sessionmaker(engine: Engine) -> sessionmaker:
    Base.metadata.create_all(engine)
    return sessionmaker(engine, expire_on_commit=False)
