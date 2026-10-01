import pytest


def test_email_code_signs_in_and_creates_the_account(world):
    t = world.sign_in("New@Mail.dev")
    assert t["user"]["email"] == "new@mail.dev"  # addresses are case-insensitive
    me = world.client.get("/v1/me", headers=world.auth(t)).json()
    assert me["email"] == "new@mail.dev"
    # signing in again is the same account
    assert world.sign_in("new@mail.dev")["user"]["id"] == t["user"]["id"]


def test_requesting_a_code_never_reveals_whether_an_account_exists(world):
    world.sign_in("known@mail.dev")
    a = world.client.post("/v1/auth/email", json={"email": "known@mail.dev"})
    b = world.client.post("/v1/auth/email", json={"email": "unknown@mail.dev"})
    assert (a.status_code, a.json()) == (b.status_code, b.json()) == (202, {"ok": True})


def test_a_code_works_once(world):
    world.client.post("/v1/auth/email", json={"email": "a@mail.dev"})
    code = world.mailer.sent[-1][1]
    assert world.client.post("/v1/auth/verify", json={"email": "a@mail.dev", "code": code}).status_code == 200
    assert world.client.post("/v1/auth/verify", json={"email": "a@mail.dev", "code": code}).status_code == 401


def test_the_code_is_forgiving_about_case_and_dashes(world):
    world.client.post("/v1/auth/email", json={"email": "a@mail.dev"})
    code = world.mailer.sent[-1][1]
    typed = code.replace("-", "").lower()
    assert world.client.post("/v1/auth/verify", json={"email": "a@mail.dev", "code": typed}).status_code == 200


def test_a_code_expires(world):
    world.client.post("/v1/auth/email", json={"email": "a@mail.dev"})
    code = world.mailer.sent[-1][1]
    world.clock.advance(16 * 60)
    assert world.client.post("/v1/auth/verify", json={"email": "a@mail.dev", "code": code}).status_code == 401


def test_guessing_is_locked_out_after_five_wrong_tries(world):
    world.client.post("/v1/auth/email", json={"email": "a@mail.dev"})
    code = world.mailer.sent[-1][1]
    for _ in range(5):
        assert world.client.post("/v1/auth/verify", json={"email": "a@mail.dev", "code": "AAAA-AAAA"}).status_code == 401
    # even the right code no longer works: a new one has to be requested
    assert world.client.post("/v1/auth/verify", json={"email": "a@mail.dev", "code": code}).status_code == 401


def test_a_new_code_replaces_the_old_one(world):
    world.client.post("/v1/auth/email", json={"email": "a@mail.dev"})
    first = world.mailer.sent[-1][1]
    world.client.post("/v1/auth/email", json={"email": "a@mail.dev"})
    second = world.mailer.sent[-1][1]
    assert world.client.post("/v1/auth/verify", json={"email": "a@mail.dev", "code": first}).status_code == 401
    world.client.post("/v1/auth/email", json={"email": "a@mail.dev"})
    assert world.client.post("/v1/auth/verify", json={"email": "a@mail.dev", "code": second}).status_code == 401  # replaced again
    third = world.mailer.sent[-1][1]
    assert world.client.post("/v1/auth/verify", json={"email": "a@mail.dev", "code": third}).status_code == 200


def test_requesting_codes_is_rate_limited_per_address(world):
    codes = [world.client.post("/v1/auth/email", json={"email": "spam@mail.dev"}).status_code for _ in range(7)]
    assert codes[:5] == [202] * 5 and codes[5] == 429


def test_invalid_email_is_rejected(world):
    assert world.client.post("/v1/auth/email", json={"email": "not-an-email"}).status_code == 422


def test_mail_failure_is_a_clean_error(world):
    def boom(*a, **k):
        raise OSError("smtp down")

    world.mailer.send_code = boom
    r = world.client.post("/v1/auth/email", json={"email": "a@mail.dev"})
    assert r.status_code == 502 and "smtp" not in r.text


# ---- access and refresh tokens -------------------------------------------------------------------------

def test_endpoints_need_a_valid_access_token(world):
    assert world.client.get("/v1/me").status_code == 401
    assert world.client.get("/v1/me", headers={"Authorization": "Bearer garbage"}).status_code == 401
    t = world.sign_in()
    assert world.client.get("/v1/me", headers=world.auth(t)).status_code == 200
    world.clock.advance(16 * 60)
    assert world.client.get("/v1/me", headers=world.auth(t)).status_code == 401, "access tokens last 15 minutes"


def test_refresh_rotates_the_token(world):
    t = world.sign_in()
    r = world.client.post("/v1/auth/refresh", json={"refresh_token": t["refresh_token"]})
    assert r.status_code == 200
    new = r.json()
    assert new["refresh_token"] != t["refresh_token"]
    assert world.client.get("/v1/me", headers=world.auth(new)).status_code == 200


def test_a_reused_refresh_token_ends_the_session(world):
    t = world.sign_in()
    new = world.client.post("/v1/auth/refresh", json={"refresh_token": t["refresh_token"]}).json()
    # someone presents the OLD token again: it was copied, so the whole session is revoked
    assert world.client.post("/v1/auth/refresh", json={"refresh_token": t["refresh_token"]}).status_code == 401
    assert world.client.post("/v1/auth/refresh", json={"refresh_token": new["refresh_token"]}).status_code == 401


def test_logout_revokes_the_refresh_token(world):
    t = world.sign_in()
    assert world.client.post("/v1/auth/logout", json={"refresh_token": t["refresh_token"]}).status_code == 204
    assert world.client.post("/v1/auth/refresh", json={"refresh_token": t["refresh_token"]}).status_code == 401


def test_refresh_tokens_expire(world):
    t = world.sign_in()
    world.clock.advance(61 * 86400)
    assert world.client.post("/v1/auth/refresh", json={"refresh_token": t["refresh_token"]}).status_code == 401


# ---- Google and Apple ------------------------------------------------------------------------------------

@pytest.mark.parametrize("provider", ["google", "apple"])
def test_provider_sign_in_creates_and_reuses_the_account(world, provider):
    t = world.client.post("/v1/auth/oidc", json={"provider": provider, "id_token": world.id_token(provider)}).json()
    assert t["user"]["email"] == "oidc@mail.dev"
    again = world.client.post("/v1/auth/oidc", json={"provider": provider, "id_token": world.id_token(provider)}).json()
    assert again["user"]["id"] == t["user"]["id"]


def test_a_provider_sign_in_joins_the_account_with_the_same_verified_email(world):
    emailed = world.sign_in("oidc@mail.dev")
    t = world.client.post("/v1/auth/oidc", json={"provider": "google", "id_token": world.id_token("google")}).json()
    assert t["user"]["id"] == emailed["user"]["id"]


@pytest.mark.parametrize(
    "why,overrides",
    [
        ("wrong audience", {"aud": "someone-elses-app"}),
        ("wrong issuer", {"iss": "https://evil.example"}),
        ("expired", {"exp": 1, "iat": 0}),
        ("email not verified", {"email_verified": False}),
        ("no email", {"email": None}),
        ("no subject", {"sub": None}),
    ],
)
def test_bad_provider_tokens_are_refused(world, why, overrides):
    r = world.client.post("/v1/auth/oidc", json={"provider": "google", "id_token": world.id_token("google", **overrides)})
    assert r.status_code == 401, why


def test_a_token_signed_by_another_key_is_refused(world):
    from cryptography.hazmat.primitives.asymmetric import rsa
    import jwt
    import time

    other = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    now = int(time.time())
    forged = jwt.encode({"iss": "https://accounts.google.com", "aud": "google-client-id.apps.googleusercontent.com", "sub": "s", "email": "x@mail.dev", "email_verified": True, "iat": now, "exp": now + 600}, other, algorithm="RS256")
    assert world.client.post("/v1/auth/oidc", json={"provider": "google", "id_token": forged}).status_code == 401


def test_nonce_must_match_when_given(world):
    tok = world.id_token("apple", nonce="abc")
    assert world.client.post("/v1/auth/oidc", json={"provider": "apple", "id_token": tok, "nonce": "other"}).status_code == 401
    assert world.client.post("/v1/auth/oidc", json={"provider": "apple", "id_token": tok, "nonce": "abc"}).status_code == 200


def test_provider_sign_in_is_off_when_not_configured(world):
    from app.oidc import OidcVerifier

    world.app.router  # keep the app alive
    v = OidcVerifier([], [])
    assert not v.enabled("google") and not v.enabled("apple") and not v.enabled("nope")


def test_deleting_the_account_removes_everything(world):
    t = world.sign_in()
    d = world.new_device()
    world.client.post("/v1/devices", json=world.body(d), headers=world.auth(t))
    assert world.client.delete("/v1/me", headers=world.auth(t)).status_code == 204
    assert world.client.get("/v1/me", headers=world.auth(t)).status_code == 401
    assert world.client.post("/v1/auth/refresh", json={"refresh_token": t["refresh_token"]}).status_code == 401
    # the email can sign up again as a brand-new account with no devices
    t2 = world.sign_in()
    assert t2["user"]["id"] != t["user"]["id"]
    assert world.client.get("/v1/devices", headers=world.auth(t2)).json() == {"devices": []}
