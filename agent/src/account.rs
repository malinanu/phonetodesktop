//! Optional account: a PC that has joined an account accepts that account's phones without a QR code.
//!
//! The phone presents a device certificate signed by the account service; checking it is only checking an
//! Ed25519 signature against the service key the PC cached when it joined, so it works with no internet.
//! Format and trust model: docs/ACCOUNTS.md. Everything here is pure (no sockets, no clock), so it is unit
//! tested, including against certificates made by the real Python service (see the fixtures in the tests).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const CERT_PREFIX: &[u8] = b"PRv2-cert\0";
const ENVELOPE_PREFIX: &[u8] = b"PRv2-signed\0";
/// A certificate "issued" slightly in the future is still fine (clocks differ).
const SKEW_S: u64 = 300;

/// What a PC remembers about the account it joined. Stored in the agent config.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct AccountTrust {
    /// Base address of the account service, e.g. `https://files.example.com/account`.
    pub url: String,
    /// The account's id (random, unguessable).
    pub acct: String,
    /// The service's public keys by key id (base64url Ed25519), learned from `/.well-known/phoneremote.json`.
    pub keys: BTreeMap<String, String>,
    /// How long after expiry a certificate is still accepted, so a phone that was away for a while still works.
    #[serde(default = "default_grace_days")]
    pub grace_days: u64,
    /// May this account's phones use the mouse and keyboard? Default yes (they are the owner's phones).
    #[serde(default = "yes")]
    pub input_allowed: bool,
    /// Devices the owner removed in the account, from the last signed list this PC fetched.
    #[serde(default)]
    pub revoked: Vec<Revoked>,
    /// Unix time of the last successful revocation fetch (0 = never).
    #[serde(default)]
    pub revoked_checked: u64,
}

fn default_grace_days() -> u64 {
    14
}
fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Revoked {
    pub dev: String,
    /// Unix seconds when it was removed.
    pub at: u64,
}

#[derive(Debug, PartialEq)]
pub enum CertError {
    Malformed,
    Version,
    UnknownKey,
    BadSignature,
    NotYetValid,
    Expired,
    WrongAccount,
    Revoked,
}

/// The fields of a certificate that a PC cares about.
#[derive(Debug, Clone, PartialEq)]
pub struct Cert {
    pub acct: String,
    pub dev: String,
    pub kind: String,
    pub name: String,
    pub pk: VerifyingKey,
    pub iat: u64,
    pub exp: u64,
}

fn key_from_b64(s: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = URL_SAFE_NO_PAD.decode(s).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

fn check_signed(prefix: &[u8], body: &[u8], sig: &[u8], key_b64: &str) -> Result<(), CertError> {
    let key = key_from_b64(key_b64).ok_or(CertError::UnknownKey)?;
    let sig = Signature::from_slice(sig).map_err(|_| CertError::BadSignature)?;
    let mut msg = prefix.to_vec();
    msg.extend_from_slice(body);
    key.verify_strict(&msg, &sig).map_err(|_| CertError::BadSignature)
}

impl AccountTrust {
    /// Verify a device certificate: signature by a key we trust, dates (with the grace period), and that it is
    /// this account's and has not been revoked. `now` is unix seconds.
    pub fn verify_cert(&self, cert: &str, now: u64) -> Result<Cert, CertError> {
        let (body_b64, sig_b64) = cert.split_once('.').ok_or(CertError::Malformed)?;
        let body = URL_SAFE_NO_PAD.decode(body_b64).map_err(|_| CertError::Malformed)?;
        let sig = URL_SAFE_NO_PAD.decode(sig_b64).map_err(|_| CertError::Malformed)?;
        let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| CertError::Malformed)?;
        if v.get("v").and_then(|x| x.as_u64()) != Some(1) {
            return Err(CertError::Version);
        }
        let kid = v.get("kid").and_then(|x| x.as_str()).ok_or(CertError::Malformed)?;
        let key = self.keys.get(kid).ok_or(CertError::UnknownKey)?;
        check_signed(CERT_PREFIX, &body, &sig, key)?;
        let text = |f: &str| v.get(f).and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(str::to_owned).ok_or(CertError::Malformed);
        let (iat, exp) = (v.get("iat").and_then(|x| x.as_u64()).ok_or(CertError::Malformed)?, v.get("exp").and_then(|x| x.as_u64()).ok_or(CertError::Malformed)?);
        if iat > now + SKEW_S {
            return Err(CertError::NotYetValid);
        }
        if exp + self.grace_days * 86_400 < now {
            return Err(CertError::Expired);
        }
        let cert = Cert {
            acct: text("acct")?,
            dev: text("dev")?,
            kind: text("kind")?,
            name: v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_owned(),
            pk: key_from_b64(&text("pk")?).ok_or(CertError::Malformed)?,
            iat,
            exp,
        };
        if cert.acct != self.acct {
            return Err(CertError::WrongAccount);
        }
        // Removed after this certificate was issued. A device re-enrolled later gets a newer `iat` and passes.
        if self.revoked.iter().any(|r| r.dev == cert.dev && r.at >= cert.iat) {
            return Err(CertError::Revoked);
        }
        Ok(cert)
    }

    /// Replace the stored revocation list with a freshly fetched signed one. Returns false (and changes nothing)
    /// if the signature is bad or the list is for another account.
    pub fn apply_revocations(&mut self, envelope: &str, now: u64) -> bool {
        match self.parse_revocations(envelope) {
            Some(list) => {
                self.revoked = list;
                self.revoked_checked = now;
                true
            }
            None => false,
        }
    }

    fn parse_revocations(&self, envelope: &str) -> Option<Vec<Revoked>> {
        let env: serde_json::Value = serde_json::from_str(envelope).ok()?;
        let body = URL_SAFE_NO_PAD.decode(env.get("payload")?.as_str()?).ok()?;
        let sig = URL_SAFE_NO_PAD.decode(env.get("sig")?.as_str()?).ok()?;
        let key = self.keys.get(env.get("kid")?.as_str()?)?;
        check_signed(ENVELOPE_PREFIX, &body, &sig, key).ok()?;
        let payload: serde_json::Value = serde_json::from_slice(&body).ok()?;
        if payload.get("acct")?.as_str()? != self.acct {
            return None;
        }
        payload.get("revoked")?.as_array()?.iter().map(|r| Some(Revoked { dev: r.get("dev")?.as_str()?.to_owned(), at: r.get("at")?.as_u64()? })).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    // Made by the real service code (account/app/certs.py) with these fixed inputs; regenerate with testdata/make_account_fixtures.py.
    const SERVICE_PK: &str = include_str!("../testdata/account_service_pk.txt");
    const FIXTURE_CERT: &str = include_str!("../testdata/account_cert.txt");
    const FIXTURE_REVOCATIONS: &str = include_str!("../testdata/account_revocations.json");

    fn trust() -> AccountTrust {
        AccountTrust {
            url: "https://example.test/account".into(),
            acct: "a".repeat(32),
            keys: BTreeMap::from([("k1".to_string(), SERVICE_PK.trim().to_string())]),
            grace_days: 14,
            input_allowed: true,
            revoked: vec![],
            revoked_checked: 0,
        }
    }

    #[test]
    fn a_certificate_made_by_the_python_service_verifies() {
        let c = trust().verify_cert(FIXTURE_CERT.trim(), 1_800_000_000).expect("valid");
        assert_eq!(c.dev, "phone-1");
        assert_eq!(c.kind, "phone");
        assert_eq!(c.name, "Pixel");
        assert_eq!(c.iat, 1_799_999_000);
    }

    #[test]
    fn the_signed_revocation_list_from_the_service_is_accepted_and_applies() {
        let mut t = trust();
        assert!(t.apply_revocations(FIXTURE_REVOCATIONS.trim(), 1_800_000_100));
        assert_eq!(t.revoked, vec![Revoked { dev: "phone-1".into(), at: 1_800_000_050 }]);
        assert_eq!(t.revoked_checked, 1_800_000_100);
        assert_eq!(t.verify_cert(FIXTURE_CERT.trim(), 1_800_000_200), Err(CertError::Revoked));
    }

    // A tiny issuer for the cases the fixture cannot cover.
    fn issue(sk: &SigningKey, kid: &str, mut f: serde_json::Value) -> String {
        f["v"] = 1.into();
        f["kid"] = kid.into();
        let body = serde_json::to_vec(&f).unwrap();
        let mut msg = CERT_PREFIX.to_vec();
        msg.extend_from_slice(&body);
        format!("{}.{}", URL_SAFE_NO_PAD.encode(&body), URL_SAFE_NO_PAD.encode(sk.sign(&msg).to_bytes()))
    }

    fn setup() -> (AccountTrust, SigningKey, String) {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let phone = SigningKey::from_bytes(&[9u8; 32]);
        let mut t = trust();
        t.keys.insert("k2".into(), URL_SAFE_NO_PAD.encode(sk.verifying_key().to_bytes()));
        (t, sk, URL_SAFE_NO_PAD.encode(phone.verifying_key().to_bytes()))
    }

    fn payload(t: &AccountTrust, pk: &str, iat: u64, exp: u64) -> serde_json::Value {
        serde_json::json!({"acct": t.acct, "dev": "d1", "kind": "phone", "name": "N", "pk": pk, "iat": iat, "exp": exp})
    }

    #[test]
    fn dates_signature_account_and_keys_are_all_checked() {
        let (t, sk, pk) = setup();
        let now = 1_000_000_000u64;
        let ok = issue(&sk, "k2", payload(&t, &pk, now - 10, now + 1000));
        assert!(t.verify_cert(&ok, now).is_ok());

        // expired, but within the grace period; then beyond it
        let old = issue(&sk, "k2", payload(&t, &pk, now - 100_000, now - 10));
        assert!(t.verify_cert(&old, now).is_ok());
        assert_eq!(t.verify_cert(&old, now + 15 * 86_400), Err(CertError::Expired));
        // issued in the future
        let future = issue(&sk, "k2", payload(&t, &pk, now + 10_000, now + 20_000));
        assert_eq!(t.verify_cert(&future, now), Err(CertError::NotYetValid));
        // another account
        let mut other = payload(&t, &pk, now - 10, now + 1000);
        other["acct"] = "b".repeat(32).into();
        assert_eq!(t.verify_cert(&issue(&sk, "k2", other), now), Err(CertError::WrongAccount));
        // a key we do not know, and a key id that is not ours
        let stranger = SigningKey::from_bytes(&[8u8; 32]);
        assert_eq!(t.verify_cert(&issue(&stranger, "k2", payload(&t, &pk, now - 10, now + 1000)), now), Err(CertError::BadSignature));
        assert_eq!(t.verify_cert(&issue(&sk, "k9", payload(&t, &pk, now - 10, now + 1000)), now), Err(CertError::UnknownKey));
        // tampering after signing
        let (b, s) = ok.split_once('.').unwrap();
        let mut body = URL_SAFE_NO_PAD.decode(b).unwrap();
        let at = body.iter().position(|&c| c == b'N').unwrap();
        body[at] = b'M';
        assert_eq!(t.verify_cert(&format!("{}.{s}", URL_SAFE_NO_PAD.encode(body)), now), Err(CertError::BadSignature));
    }

    #[test]
    fn garbage_never_panics() {
        let t = trust();
        for bad in ["", ".", "a.b", "????.????", "e30.e30", &"x".repeat(5000), "e30"] {
            assert!(t.verify_cert(bad, 1).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_device_enrolled_again_after_removal_is_accepted() {
        let (mut t, sk, pk) = setup();
        let now = 1_000_000_000u64;
        t.revoked.push(Revoked { dev: "d1".into(), at: now - 500 });
        let before = issue(&sk, "k2", payload(&t, &pk, now - 1000, now + 1000));
        let after = issue(&sk, "k2", payload(&t, &pk, now - 100, now + 1000));
        assert_eq!(t.verify_cert(&before, now), Err(CertError::Revoked));
        assert!(t.verify_cert(&after, now).is_ok());
    }

    #[test]
    fn a_revocation_list_must_be_signed_by_a_known_key_for_this_account() {
        let mut t = trust();
        let before = t.clone();
        // wrong signature
        let bad = FIXTURE_REVOCATIONS.trim().replace("\"sig\":\"", "\"sig\":\"A");
        assert!(!t.apply_revocations(&bad, 5));
        assert!(!t.apply_revocations("not json", 5));
        // another account's list
        t.acct = "c".repeat(32);
        assert!(!t.apply_revocations(FIXTURE_REVOCATIONS.trim(), 5));
        t.acct = before.acct.clone();
        assert_eq!(t.revoked, before.revoked, "a rejected list changes nothing");
        assert_eq!(t.revoked_checked, 0);
    }
}
