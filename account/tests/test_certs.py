import pytest

from app import certs

SEED = bytes([7]) * 32
PUB = {"k1": certs.public_key_bytes(SEED)}
PAYLOAD = {"acct": "a" * 32, "dev": "device-0001", "kind": "phone", "name": "Pixel", "pk": "AAAA", "iat": 1000, "exp": 2000}


def test_a_valid_certificate_verifies():
    c = certs.issue(SEED, "k1", PAYLOAD)
    p = certs.verify(c, PUB, now=1500)
    assert (p["dev"], p["kind"], p["acct"], p["kid"], p["v"]) == ("device-0001", "phone", "a" * 32, "k1", 1)


def test_expiry_and_grace():
    c = certs.issue(SEED, "k1", PAYLOAD)
    with pytest.raises(certs.CertError, match="expired"):
        certs.verify(c, PUB, now=2001)
    assert certs.verify(c, PUB, now=2500, grace=600)["dev"] == "device-0001"  # a PC may accept a recently expired one
    with pytest.raises(certs.CertError, match="expired"):
        certs.verify(c, PUB, now=2700, grace=600)


def test_not_yet_valid_beyond_clock_skew():
    c = certs.issue(SEED, "k1", {**PAYLOAD, "iat": 10_000, "exp": 20_000})
    with pytest.raises(certs.CertError, match="not valid yet"):
        certs.verify(c, PUB, now=1000)
    assert certs.verify(c, PUB, now=9_800)  # within 300 s of skew


def test_tampering_is_detected():
    c = certs.issue(SEED, "k1", PAYLOAD)
    body, sig = c.split(".")
    from app.security import b64u, b64u_decode

    forged = b64u(b64u_decode(body).replace(b"device-0001", b"device-0002"))
    for bad in (f"{forged}.{sig}", f"{body}.{sig[:-2]}AA", f"{sig}.{body}"):
        with pytest.raises(certs.CertError):
            certs.verify(bad, PUB, now=1500)


def test_wrong_or_unknown_key():
    c = certs.issue(SEED, "k1", PAYLOAD)
    with pytest.raises(certs.CertError, match="bad signature"):
        certs.verify(c, {"k1": certs.public_key_bytes(bytes([8]) * 32)}, now=1500)
    with pytest.raises(certs.CertError, match="unknown key"):
        certs.verify(c, {"other": PUB["k1"]}, now=1500)


def test_garbage_is_malformed_never_a_crash():
    for bad in ("", ".", "a.b", "not a cert", "e30.e30", "e30=.e30="):
        with pytest.raises(certs.CertError):
            certs.verify(bad, PUB, now=1500)


def test_missing_fields_are_rejected():
    c = certs.issue(SEED, "k1", {k: v for k, v in PAYLOAD.items() if k != "dev"})
    with pytest.raises(certs.CertError, match="missing dev"):
        certs.verify(c, PUB, now=1500)


def test_envelopes_are_signed():
    env = certs.sign_envelope(SEED, "k1", {"revoked": []})
    assert certs.verify_envelope(env, PUB) == {"revoked": []}
    env["payload"] = env["payload"][:-2] + "AA"
    with pytest.raises(certs.CertError):
        certs.verify_envelope(env, PUB)
