from fastapi.testclient import TestClient

from app.main import create_app

SITE = "https://lanyard.3logiq.com"


def client(origins):
    from app.config import Settings

    s = Settings(database_url="sqlite://", signing_keys={"k1": bytes([1]) * 32}, jwt_secret="x" * 40, cors_origins=origins)
    return TestClient(create_app(s))


def test_the_website_may_call_the_service_and_nobody_else():
    c = client([SITE])
    ok = c.options("/v1/devices", headers={"Origin": SITE, "Access-Control-Request-Method": "GET", "Access-Control-Request-Headers": "authorization"})
    assert ok.headers.get("access-control-allow-origin") == SITE
    other = c.options("/v1/devices", headers={"Origin": "https://evil.example", "Access-Control-Request-Method": "GET"})
    assert "access-control-allow-origin" not in other.headers
    assert "access-control-allow-credentials" not in ok.headers


def test_no_cors_headers_unless_configured():
    c = client([])
    r = c.get("/.well-known/phoneremote.json", headers={"Origin": SITE})
    assert r.status_code == 200 and "access-control-allow-origin" not in r.headers
