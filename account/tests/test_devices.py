from app import certs
from app.security import b64u


def enroll(world, tokens, device):
    return world.client.post("/v1/devices", json=world.body(device), headers=world.auth(tokens))


def test_enrolling_returns_a_certificate_the_service_key_signed(world):
    t = world.sign_in()
    d = world.new_device("Pixel 8")
    r = enroll(world, t, d)
    assert r.status_code == 201
    out = r.json()
    p = certs.verify(out["cert"], world.public_keys, now=int(world.clock()))
    assert (p["dev"], p["pk"], p["kind"], p["name"], p["acct"]) == (d["id"], d["pubkey"], "phone", "Pixel 8", t["user"]["id"])
    assert out["account_id"] == t["user"]["id"]
    assert p["exp"] - p["iat"] == 30 * 86400
    assert {k["kid"] for k in out["keys"]["keys"]} == {"k1", "k0"} and out["keys"]["current"] == "k1"


def test_certificates_are_signed_by_the_current_key_only(world):
    t = world.sign_in()
    cert = enroll(world, t, world.new_device()).json()["cert"]
    assert certs.verify(cert, world.public_keys, now=int(world.clock()))["kid"] == "k1"
    only_old = {"k0": world.public_keys["k0"]}
    import pytest

    with pytest.raises(certs.CertError):
        certs.verify(cert, only_old, now=int(world.clock()))


def test_enrolling_twice_is_idempotent(world):
    t = world.sign_in()
    d = world.new_device()
    assert enroll(world, t, d).status_code == 201
    assert enroll(world, t, d).status_code == 201
    assert len(world.client.get("/v1/devices", headers=world.auth(t)).json()["devices"]) == 1


def test_a_device_id_cannot_be_taken_over_by_another_account(world):
    a, b = world.sign_in("a@mail.dev"), world.sign_in("b@mail.dev")
    d = world.new_device()
    assert enroll(world, a, d).status_code == 201
    assert enroll(world, b, d).status_code == 409
    other_key = world.new_device(device_id=d["id"])  # same id, different key
    assert enroll(world, a, other_key).status_code == 409


def test_one_key_cannot_be_enrolled_under_two_ids(world):
    t = world.sign_in()
    d = world.new_device()
    enroll(world, t, d)
    assert enroll(world, t, {**d, "id": "another-id-0001"}).status_code == 409


def test_invalid_devices_are_rejected(world):
    t = world.sign_in()
    d = world.new_device()
    for patch in ({"id": "short"}, {"id": "has space in it!"}, {"name": ""}, {"name": "   "}, {"platform": "Android!"}, {"kind": "tv"}, {"pubkey": "AAAA"}, {"pubkey": b64u(b"\x00" * 31)}):
        assert enroll(world, t, {**d, **patch}).status_code == 422, patch


def test_you_only_see_and_change_your_own_devices(world):
    a, b = world.sign_in("a@mail.dev"), world.sign_in("b@mail.dev")
    d = world.new_device()
    enroll(world, a, d)
    assert world.client.get("/v1/devices", headers=world.auth(b)).json() == {"devices": []}
    assert world.client.patch(f"/v1/devices/{d['id']}", json={"name": "mine"}, headers=world.auth(b)).status_code == 404
    assert world.client.delete(f"/v1/devices/{d['id']}", headers=world.auth(b)).status_code == 404
    assert world.client.post(f"/v1/devices/{d['id']}/cert", headers=world.auth(b)).status_code == 404


def test_rename(world):
    t = world.sign_in()
    d = world.new_device("Old")
    enroll(world, t, d)
    assert world.client.patch(f"/v1/devices/{d['id']}", json={"name": "New name"}, headers=world.auth(t)).json()["name"] == "New name"
    assert world.client.patch(f"/v1/devices/{d['id']}", json={"name": "  "}, headers=world.auth(t)).status_code == 422


def test_renewing_gives_a_fresh_certificate(world):
    t = world.sign_in()
    d = world.new_device()
    first = enroll(world, t, d).json()["cert"]
    world.clock.advance(20 * 86400)
    second = world.client.post(f"/v1/devices/{d['id']}/cert", headers=world.auth(world.client.post("/v1/auth/refresh", json={"refresh_token": t["refresh_token"]}).json())).json()["cert"]
    p1 = certs.verify(first, world.public_keys, now=int(world.clock()))
    p2 = certs.verify(second, world.public_keys, now=int(world.clock()))
    assert p2["exp"] > p1["exp"]


def test_a_removed_device_is_revoked_and_cannot_get_certificates(world):
    t = world.sign_in()
    d = world.new_device()
    enroll(world, t, d)
    assert world.client.delete(f"/v1/devices/{d['id']}", headers=world.auth(t)).status_code == 204
    assert world.client.post(f"/v1/devices/{d['id']}/cert", headers=world.auth(t)).status_code == 403
    assert enroll(world, t, d).status_code == 403, "a removed key cannot be quietly re-added"
    listed = world.client.get("/v1/devices", headers=world.auth(t)).json()["devices"]
    assert listed[0]["revoked"] is True


def test_the_revocation_list_is_signed_and_lists_removed_devices(world):
    t = world.sign_in()
    keep, drop = world.new_device("Keep"), world.new_device("Drop")
    enroll(world, t, keep)
    enroll(world, t, drop)
    acct = t["user"]["id"]
    empty = certs.verify_envelope(world.client.get(f"/v1/accounts/{acct}/revocations").json(), world.public_keys)
    assert empty["revoked"] == [] and empty["acct"] == acct
    world.clock.advance(100)
    world.client.delete(f"/v1/devices/{drop['id']}", headers=world.auth(t))
    env = world.client.get(f"/v1/accounts/{acct}/revocations").json()
    payload = certs.verify_envelope(env, world.public_keys)
    assert [r["dev"] for r in payload["revoked"]] == [drop["id"]]
    assert payload["now"] == int(world.clock())
    # a forged list does not verify
    env["payload"] = env["payload"][:-2] + "AA"
    import pytest

    with pytest.raises(certs.CertError):
        certs.verify_envelope(env, world.public_keys)


def test_revocations_for_another_account_are_not_leaked(world):
    a, b = world.sign_in("a@mail.dev"), world.sign_in("b@mail.dev")
    d = world.new_device()
    enroll(world, a, d)
    world.client.delete(f"/v1/devices/{d['id']}", headers=world.auth(a))
    payload = certs.verify_envelope(world.client.get(f"/v1/accounts/{b['user']['id']}/revocations").json(), world.public_keys)
    assert payload["revoked"] == []
    assert world.client.get("/v1/accounts/not-an-id/revocations").status_code == 404


def test_the_public_keys_are_published(world):
    doc = world.client.get("/.well-known/phoneremote.json").json()
    assert doc["current"] == "k1" and doc["v"] == 1
    from app.security import b64u_decode

    assert {k["kid"]: b64u_decode(k["pk"]) for k in doc["keys"]} == world.public_keys


# ---- joining a PC with a link code ---------------------------------------------------------------------------

def test_a_link_code_lets_a_pc_join_the_account_once(world):
    t = world.sign_in()
    code = world.client.post("/v1/links", headers=world.auth(t)).json()["code"]
    pc = world.new_device("Desk PC", kind="pc", platform="linux")
    r = world.client.post("/v1/links/claim", json={**world.body(pc), "code": code})
    assert r.status_code == 201
    p = certs.verify(r.json()["cert"], world.public_keys, now=int(world.clock()))
    assert (p["kind"], p["acct"], p["dev"]) == ("pc", t["user"]["id"], pc["id"])
    other = world.new_device("Second", kind="pc", platform="linux")
    assert world.client.post("/v1/links/claim", json={**world.body(other), "code": code}).status_code == 401, "single use"


def test_a_link_code_expires(world):
    t = world.sign_in()
    code = world.client.post("/v1/links", headers=world.auth(t)).json()["code"]
    world.clock.advance(11 * 60)
    pc = world.new_device(kind="pc", platform="linux")
    assert world.client.post("/v1/links/claim", json={**world.body(pc), "code": code}).status_code == 401


def test_link_codes_need_sign_in_and_wrong_codes_are_rate_limited(world):
    assert world.client.post("/v1/links").status_code == 401
    pc = world.new_device(kind="pc", platform="linux")
    codes = [world.client.post("/v1/links/claim", json={**world.body(pc), "code": "AAAA-BBBB"}).status_code for _ in range(12)]
    assert codes[:10] == [401] * 10 and codes[10] == 429
