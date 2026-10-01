//! Who may control this PC: approved phones with their own tokens, plus the legacy shared secret.
//! Pure logic (no sockets), so the pairing state machine is unit-tested.

use crate::account::{AccountTrust, CertError};
use crate::config::{self, Config, Device};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CODE_TTL: Duration = Duration::from_secs(15 * 60);
const PENDING_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, PartialEq, Clone)]
pub enum AuthErr {
    /// Wrong token for a known device, or wrong legacy secret.
    BadToken,
    /// Device id is unknown (removed from the PC). The phone should forget this PC.
    Revoked,
    /// Shared-secret logins are switched off.
    LegacyOff,
}

#[derive(Debug, PartialEq, Clone)]
pub enum PairErr {
    BadCode,
    Expired,
    /// The public key is not a valid Ed25519 key.
    BadKey,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Decision {
    Approved(String),
    Denied,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Pending {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub age_s: u64,
    pub platform: String,
}

struct PendingEntry {
    id: String,
    name: String,
    ip: String,
    at: Instant,
    /// Protocol v2: (public key, platform). `None` = a v1 phone that will be given a bearer token.
    key: Option<(String, String)>,
}

struct Inner {
    cfg: Config,
    code: String,
    code_at: Instant,
    pending: Vec<PendingEntry>,
    decisions: HashMap<String, Decision>,
    online: HashMap<String, usize>,
    /// Devices that logged in with an account certificate (not stored in the trust list), id -> name.
    account_devices: HashMap<String, String>,
}

pub struct Auth {
    inner: Mutex<Inner>,
    persist: bool,
}

pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The exact bytes a phone signs to log in (protocol v2). Bound to this PC and this device so a signature
/// cannot be replayed to another PC, and to the connection's fresh nonce so it cannot be replayed at all.
pub fn auth_message(pc_id: &str, device: &str, nonce: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(32 + pc_id.len() + device.len() + nonce.len());
    m.extend_from_slice(b"PRv2-auth\0");
    m.extend_from_slice(pc_id.as_bytes());
    m.push(0);
    m.extend_from_slice(device.as_bytes());
    m.push(0);
    m.extend_from_slice(nonce);
    m
}

/// A valid 32-byte Ed25519 public key from its base64url text.
fn parse_pubkey(pk: &str) -> Option<ed25519_dalek::VerifyingKey> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let bytes: [u8; 32] = URL_SAFE_NO_PAD.decode(pk).ok()?.try_into().ok()?;
    ed25519_dalek::VerifyingKey::from_bytes(&bytes).ok()
}

impl Auth {
    pub fn new(cfg: Config) -> Self {
        Self::build(cfg, true)
    }

    #[cfg(test)]
    pub(crate) fn in_memory(cfg: Config) -> Self {
        Self::build(cfg, false)
    }

    fn build(cfg: Config, persist: bool) -> Self {
        Auth {
            inner: Mutex::new(Inner {
                cfg,
                code: config::new_token(),
                code_at: Instant::now(),
                pending: vec![],
                decisions: HashMap::new(),
                online: HashMap::new(),
                account_devices: HashMap::new(),
            }),
            persist,
        }
    }

    fn save(&self, i: &Inner) {
        if self.persist {
            let _ = config::save(&i.cfg);
        }
    }

    // ---- pairing code (what the QR carries) ----

    /// The code shown in the QR. Short-lived: it only lets a phone *ask* to pair.
    pub fn code(&self) -> String {
        let mut i = self.inner.lock().unwrap();
        if i.code_at.elapsed() > CODE_TTL {
            i.code = config::new_token();
            i.code_at = Instant::now();
        }
        i.code.clone()
    }

    pub fn refresh_code(&self) -> String {
        let mut i = self.inner.lock().unwrap();
        i.code = config::new_token();
        i.code_at = Instant::now();
        i.code.clone()
    }

    pub fn code_age(&self) -> u64 {
        self.inner.lock().unwrap().code_at.elapsed().as_secs()
    }

    // ---- logging in ----

    pub fn check_device(&self, id: &str, token: &str) -> Result<String, AuthErr> {
        let mut i = self.inner.lock().unwrap();
        let Some(d) = i.cfg.devices.iter_mut().find(|d| d.id == id) else { return Err(AuthErr::Revoked) };
        // A key-based (v2) phone has no token: an empty one must never match an empty guess.
        if d.token.is_empty() || !ct_eq(&d.token, token) {
            return Err(AuthErr::BadToken);
        }
        d.last_seen = now_s();
        Ok(d.name.clone())
    }

    /// Protocol v2 login: the phone signed `auth_message(pc_id, device, nonce)` with its private key.
    /// Returns the device's name. Unknown device -> `Revoked` (the phone forgets this PC); wrong signature
    /// or a device without a key -> `BadToken`.
    pub fn verify_signature(&self, device: &str, nonce: &[u8], sig_b64: &str) -> Result<String, AuthErr> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let mut i = self.inner.lock().unwrap();
        let pc_id = i.cfg.pc_id.clone();
        let Some(d) = i.cfg.devices.iter_mut().find(|d| d.id == device) else { return Err(AuthErr::Revoked) };
        let Some(key) = parse_pubkey(&d.pubkey) else { return Err(AuthErr::BadToken) };
        let Some(sig) = URL_SAFE_NO_PAD.decode(sig_b64).ok().and_then(|b| ed25519_dalek::Signature::from_slice(&b).ok()) else {
            return Err(AuthErr::BadToken);
        };
        if key.verify_strict(&auth_message(&pc_id, device, nonce), &sig).is_err() {
            return Err(AuthErr::BadToken);
        }
        d.last_seen = now_s();
        Ok(d.name.clone())
    }

    /// Protocol v2 login by an account phone: it presents a device certificate signed by the account this PC
    /// joined, and signs the challenge with the key named in that certificate. Works offline.
    /// A phone that is also paired locally falls back to its local key if the certificate does not work.
    pub fn verify_account_login(&self, device: &str, cert: &str, nonce: &[u8], sig_b64: &str) -> Result<String, AuthErr> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let outcome = {
            let mut i = self.inner.lock().unwrap();
            let pc_id = i.cfg.pc_id.clone();
            match i.cfg.account.as_ref().map(|a| a.verify_cert(cert, now_s())) {
                None => Err(None),
                Some(Err(e)) => Err(Some(e)),
                Some(Ok(c)) if c.dev != device || c.kind != "phone" => Err(Some(CertError::Malformed)),
                Some(Ok(c)) => {
                    let sig = URL_SAFE_NO_PAD.decode(sig_b64).ok().and_then(|b| ed25519_dalek::Signature::from_slice(&b).ok());
                    match sig {
                        Some(sig) if c.pk.verify_strict(&auth_message(&pc_id, device, nonce), &sig).is_ok() => {
                            let name = if c.name.is_empty() { "Account phone".to_string() } else { c.name };
                            i.account_devices.insert(device.to_string(), name.clone());
                            Ok(name)
                        }
                        _ => Err(Some(CertError::BadSignature)),
                    }
                }
            }
        };
        match outcome {
            Ok(name) => Ok(name),
            Err(why) => match self.verify_signature(device, nonce, sig_b64) {
                Ok(name) => Ok(name),
                // Not paired here either: tell the phone why. Only "removed from the account" means forget this PC.
                Err(_) if why == Some(CertError::Revoked) => Err(AuthErr::Revoked),
                Err(_) => Err(AuthErr::BadToken),
            },
        }
    }

    /// The "Send files" address in effect: the one set by the user, else the one baked into the build.
    pub fn files_url(&self) -> Option<String> {
        config::files_url(&self.inner.lock().unwrap().cfg)
    }

    /// Set (or, with an empty string, clear) the "Send files" address. Only a valid https address is stored.
    pub fn set_files_url(&self, raw: &str) -> bool {
        let raw = raw.trim();
        let stored = if raw.is_empty() { String::new() } else { match config::clean_files_url(raw) { Some(u) => u, None => return false } };
        let mut i = self.inner.lock().unwrap();
        i.cfg.files_url = stored;
        self.save(&i);
        true
    }

    /// The account this PC has joined, if any.
    pub fn account(&self) -> Option<AccountTrust> {
        self.inner.lock().unwrap().cfg.account.clone()
    }

    pub fn set_account(&self, account: Option<AccountTrust>) {
        let mut i = self.inner.lock().unwrap();
        if account.is_none() {
            i.account_devices.clear();
        }
        i.cfg.account = account;
        self.save(&i);
    }

    /// Store a freshly fetched signed revocation list. False if it did not verify.
    pub fn apply_revocations(&self, envelope: &str) -> bool {
        let mut i = self.inner.lock().unwrap();
        let Some(a) = i.cfg.account.as_mut() else { return false };
        let ok = a.apply_revocations(envelope, now_s());
        if ok {
            let gone: Vec<String> = a.revoked.iter().map(|r| r.dev.clone()).collect();
            i.account_devices.retain(|d, _| !gone.contains(d));
            self.save(&i);
        }
        ok
    }

    /// Does this device log in with a key (v2)?
    #[cfg(test)]
    pub fn has_key(&self, device: &str) -> bool {
        self.inner.lock().unwrap().cfg.devices.iter().any(|d| d.id == device && !d.pubkey.is_empty())
    }

    pub fn v1_allowed(&self) -> bool {
        self.inner.lock().unwrap().cfg.allow_v1
    }

    pub fn set_v1_allowed(&self, on: bool) {
        let mut i = self.inner.lock().unwrap();
        i.cfg.allow_v1 = on;
        self.save(&i);
    }

    #[cfg(test)]
    pub fn pc_id(&self) -> String {
        self.inner.lock().unwrap().cfg.pc_id.clone()
    }

    pub fn check_legacy(&self, token: &str) -> Result<(), AuthErr> {
        let i = self.inner.lock().unwrap();
        if !i.cfg.legacy_shared_auth {
            return Err(AuthErr::LegacyOff);
        }
        if ct_eq(&i.cfg.token, token) {
            Ok(())
        } else {
            Err(AuthErr::BadToken)
        }
    }

    // ---- pairing ----

    /// A phone scanned the QR. Returns Ok(Some(token)) when the phone is already known (re-scan),
    /// Ok(None) when it now waits for the owner's approval.
    pub fn request_pairing(&self, code: &str, device: &str, name: &str, ip: &str) -> Result<Option<String>, PairErr> {
        self.request_pairing_inner(code, device, name, ip, None)
    }

    /// Protocol v2: the phone also sends its public key. `Ok(Some(""))` means "already known with this key":
    /// approved without bothering the owner and without any token.
    pub fn request_pairing_v2(&self, code: &str, device: &str, name: &str, ip: &str, pk: &str, platform: &str) -> Result<Option<String>, PairErr> {
        if parse_pubkey(pk).is_none() {
            return Err(PairErr::BadKey);
        }
        let platform: String = platform.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(16).collect();
        self.request_pairing_inner(code, device, name, ip, Some((pk.to_string(), platform)))
    }

    fn request_pairing_inner(&self, code: &str, device: &str, name: &str, ip: &str, key: Option<(String, String)>) -> Result<Option<String>, PairErr> {
        let mut i = self.inner.lock().unwrap();
        if !ct_eq(&i.code, code) {
            return Err(PairErr::BadCode);
        }
        if i.code_at.elapsed() > CODE_TTL {
            return Err(PairErr::Expired);
        }
        let name: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
        if let Some(d) = i.cfg.devices.iter_mut().find(|d| d.id == device) {
            match &key {
                // Known v1 phone scanning again: no need to bother the owner.
                None if d.pubkey.is_empty() => {
                    d.token = config::new_token();
                    d.name = name;
                    let t = d.token.clone();
                    self.save(&i);
                    return Ok(Some(t));
                }
                // Known v2 phone with the SAME key: fine. A different key is a different device claiming this id,
                // so it goes through the owner's approval like any new phone.
                Some((pk, _)) if *pk == d.pubkey => {
                    d.name = name;
                    self.save(&i);
                    return Ok(Some(String::new()));
                }
                _ => {}
            }
        }
        i.pending.retain(|p| p.id != device && p.at.elapsed() < PENDING_TTL);
        i.decisions.remove(device);
        i.pending.push(PendingEntry { id: device.into(), name, ip: ip.into(), at: Instant::now(), key });
        Ok(None)
    }

    pub fn pending(&self) -> Vec<Pending> {
        let mut i = self.inner.lock().unwrap();
        i.pending.retain(|p| p.at.elapsed() < PENDING_TTL);
        i.pending
            .iter()
            .map(|p| Pending {
                id: p.id.clone(),
                name: p.name.clone(),
                ip: p.ip.clone(),
                age_s: p.at.elapsed().as_secs(),
                platform: p.key.as_ref().map(|(_, pf)| pf.clone()).unwrap_or_default(),
            })
            .collect()
    }

    /// Owner's answer. Approving mints the phone's own token.
    pub fn decide(&self, device: &str, approve: bool) -> bool {
        let mut i = self.inner.lock().unwrap();
        let Some(pos) = i.pending.iter().position(|p| p.id == device) else { return false };
        let p = i.pending.remove(pos);
        if approve {
            // A key-based phone gets no token at all: the PC keeps only its public key.
            let (token, pubkey, platform) = match p.key {
                Some((pk, pf)) => (String::new(), pk, pf),
                None => (config::new_token(), String::new(), String::new()),
            };
            i.cfg.devices.retain(|d| d.id != p.id);
            i.cfg.devices.push(Device { id: p.id.clone(), name: p.name, token: token.clone(), created: now_s(), last_seen: now_s(), input_allowed: true, pubkey, platform });
            i.decisions.insert(p.id, Decision::Approved(token));
            self.save(&i);
        } else {
            i.decisions.insert(p.id, Decision::Denied);
        }
        true
    }

    /// The waiting phone asks: has the owner answered?
    pub fn take_decision(&self, device: &str) -> Option<Decision> {
        self.inner.lock().unwrap().decisions.remove(device)
    }

    /// Phone gave up (closed the page) while waiting.
    pub fn cancel_pending(&self, device: &str) {
        self.inner.lock().unwrap().pending.retain(|p| p.id != device);
    }

    // ---- management ----

    pub fn revoke(&self, device: &str) -> bool {
        let mut i = self.inner.lock().unwrap();
        let before = i.cfg.devices.len();
        i.cfg.devices.retain(|d| d.id != device);
        let changed = i.cfg.devices.len() != before;
        if changed {
            self.save(&i);
        }
        changed
    }

    /// Live check (so the dashboard switch takes effect at once). Shared-secret phones cannot be
    /// identified, so they never get mouse and keyboard.
    pub fn input_allowed(&self, device: &str) -> bool {
        let i = self.inner.lock().unwrap();
        match i.cfg.devices.iter().find(|d| d.id == device) {
            Some(d) => d.input_allowed,
            None => i.account_devices.contains_key(device) && i.cfg.account.as_ref().is_some_and(|a| a.input_allowed),
        }
    }

    pub fn set_input_allowed(&self, device: &str, on: bool) -> bool {
        let mut i = self.inner.lock().unwrap();
        let Some(d) = i.cfg.devices.iter_mut().find(|d| d.id == device) else { return false };
        d.input_allowed = on;
        self.save(&i);
        true
    }

    pub fn revoke_all(&self) {
        let mut i = self.inner.lock().unwrap();
        i.cfg.devices.clear();
        i.cfg.token = config::new_token();
        i.cfg.legacy_shared_auth = false;
        self.save(&i);
    }

    pub fn set_legacy(&self, on: bool) {
        let mut i = self.inner.lock().unwrap();
        i.cfg.legacy_shared_auth = on;
        self.save(&i);
    }

    pub fn setup_done(&self) -> bool {
        self.inner.lock().unwrap().cfg.setup_done
    }

    pub fn mark_setup_done(&self) {
        let mut i = self.inner.lock().unwrap();
        i.cfg.setup_done = true;
        self.save(&i);
    }

    pub fn legacy_enabled(&self) -> bool {
        self.inner.lock().unwrap().cfg.legacy_shared_auth
    }

    pub fn devices(&self) -> Vec<(Device, bool)> {
        let i = self.inner.lock().unwrap();
        i.cfg.devices.iter().map(|d| (d.clone(), i.online.get(&d.id).copied().unwrap_or(0) > 0)).collect()
    }

    pub fn set_online(&self, device: &str, up: bool) {
        let mut i = self.inner.lock().unwrap();
        let n = i.online.entry(device.to_string()).or_insert(0);
        if up {
            *n += 1;
        } else {
            *n = n.saturating_sub(1);
        }
    }

    pub fn online_count(&self) -> usize {
        self.inner.lock().unwrap().online.values().filter(|n| **n > 0).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(legacy: bool) -> Auth {
        let mut c = Config {
            pc_id: "pc".into(),
            token: "legacy-secret".into(),
            devices: vec![],
            legacy_shared_auth: legacy,
            setup_done: true,
            port: 1,
            local_secret: String::new(),
            autostart_initialized: true,
            files_url: String::new(),
            allow_v1: true,
            account: None,
        };
        c.devices.clear();
        Auth::in_memory(c)
    }

    #[test]
    fn pair_approve_then_login() {
        let a = auth(false);
        let code = a.code();
        assert_eq!(a.request_pairing(&code, "phone1", "Pixel 7", "192.168.1.9"), Ok(None));
        assert_eq!(a.pending().len(), 1);
        assert_eq!(a.take_decision("phone1"), None); // still waiting
        assert!(a.decide("phone1", true));
        let Some(Decision::Approved(token)) = a.take_decision("phone1") else { panic!("not approved") };
        assert_eq!(a.check_device("phone1", &token), Ok("Pixel 7".into()));
        assert_eq!(a.check_device("phone1", "wrong"), Err(AuthErr::BadToken));
        assert!(a.pending().is_empty());
    }

    #[test]
    fn deny_and_bad_code() {
        let a = auth(false);
        assert_eq!(a.request_pairing("nope", "p", "x", "ip"), Err(PairErr::BadCode));
        let code = a.code();
        a.request_pairing(&code, "p", "x", "ip").unwrap();
        a.decide("p", false);
        assert_eq!(a.take_decision("p"), Some(Decision::Denied));
        assert_eq!(a.check_device("p", "t"), Err(AuthErr::Revoked));
    }

    #[test]
    fn revoke_one_keeps_others() {
        let a = auth(false);
        let code = a.code();
        for id in ["a", "b"] {
            a.request_pairing(&code, id, id, "ip").unwrap();
            a.decide(id, true);
        }
        let tb = match a.take_decision("b") { Some(Decision::Approved(t)) => t, _ => panic!() };
        assert!(a.revoke("a"));
        assert_eq!(a.check_device("a", "x"), Err(AuthErr::Revoked));
        assert!(a.check_device("b", &tb).is_ok());
    }

    #[test]
    fn rescan_of_known_phone_needs_no_approval() {
        let a = auth(false);
        let code = a.code();
        a.request_pairing(&code, "p", "Old name", "ip").unwrap();
        a.decide("p", true);
        let Ok(Some(t2)) = a.request_pairing(&code, "p", "New name", "ip") else { panic!("should auto-pair") };
        assert_eq!(a.check_device("p", &t2), Ok("New name".into()));
    }

    #[test]
    fn legacy_switch() {
        let a = auth(true);
        assert_eq!(a.check_legacy("legacy-secret"), Ok(()));
        assert_eq!(a.check_legacy("x"), Err(AuthErr::BadToken));
        a.set_legacy(false);
        assert_eq!(a.check_legacy("legacy-secret"), Err(AuthErr::LegacyOff));
    }

    #[test]
    fn revoke_all_rotates_legacy_secret_and_turns_it_off() {
        let a = auth(true);
        a.revoke_all();
        assert_eq!(a.check_legacy("legacy-secret"), Err(AuthErr::LegacyOff));
        assert!(!a.legacy_enabled());
    }

    #[test]
    fn input_permission_is_per_phone_and_live() {
        let a = auth(false);
        let code = a.code();
        a.request_pairing(&code, "p", "x", "ip").unwrap();
        a.decide("p", true);
        assert!(a.input_allowed("p"), "on by default");
        assert!(a.set_input_allowed("p", false));
        assert!(!a.input_allowed("p"));
        assert!(!a.input_allowed("unknown"));
        assert!(!a.set_input_allowed("unknown", true));
    }

    #[test]
    fn online_counting() {
        let a = auth(false);
        a.set_online("p", true);
        a.set_online("p", true);
        a.set_online("p", false);
        assert_eq!(a.online_count(), 1);
        a.set_online("p", false);
        assert_eq!(a.online_count(), 0);
    }

    // ---- protocol v2 (device keys) ----

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use ed25519_dalek::{Signer, SigningKey};

    fn keypair(seed: u8) -> (SigningKey, String) {
        let sk = SigningKey::from_bytes(&[seed; 32]);
        let pk = URL_SAFE_NO_PAD.encode(sk.verifying_key().to_bytes());
        (sk, pk)
    }

    fn sign(sk: &SigningKey, pc: &str, device: &str, nonce: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(sk.sign(&auth_message(pc, device, nonce)).to_bytes())
    }

    /// An Auth with one approved key-based phone.
    fn with_v2_phone() -> (Auth, SigningKey) {
        let a = auth(false);
        let (sk, pk) = keypair(7);
        let code = a.code();
        assert_eq!(a.request_pairing_v2(&code, "ph", "Pixel", "ip", &pk, "android"), Ok(None));
        assert!(a.decide("ph", true));
        (a, sk)
    }

    #[test]
    fn v2_pairing_stores_the_key_and_no_token() {
        let a = auth(false);
        let (_, pk) = keypair(1);
        let code = a.code();
        a.request_pairing_v2(&code, "ph", "Pixel 7", "ip", &pk, "android").unwrap();
        assert_eq!(a.pending()[0].platform, "android");
        assert!(a.decide("ph", true));
        let Some(Decision::Approved(token)) = a.take_decision("ph") else { panic!("not approved") };
        assert_eq!(token, "", "a key-based phone must never be given a token");
        let (d, _) = a.devices().into_iter().next().unwrap();
        assert_eq!((d.pubkey.as_str(), d.token.as_str(), d.platform.as_str()), (pk.as_str(), "", "android"));
        assert!(a.has_key("ph"));
    }

    #[test]
    fn a_valid_signature_logs_in() {
        let (a, sk) = with_v2_phone();
        let nonce = [9u8; 32];
        assert_eq!(a.verify_signature("ph", &nonce, &sign(&sk, &a.pc_id(), "ph", &nonce)), Ok("Pixel".into()));
    }

    #[test]
    fn signatures_are_bound_to_nonce_pc_and_device() {
        let (a, sk) = with_v2_phone();
        let nonce = [9u8; 32];
        let good = sign(&sk, &a.pc_id(), "ph", &nonce);
        // another connection's nonce: replay
        assert_eq!(a.verify_signature("ph", &[8u8; 32], &good), Err(AuthErr::BadToken));
        // signed for a different PC
        assert_eq!(a.verify_signature("ph", &nonce, &sign(&sk, "other-pc", "ph", &nonce)), Err(AuthErr::BadToken));
        // signed for a different device id
        assert_eq!(a.verify_signature("ph", &nonce, &sign(&sk, &a.pc_id(), "someone", &nonce)), Err(AuthErr::BadToken));
        // someone else's key
        let (other, _) = keypair(99);
        assert_eq!(a.verify_signature("ph", &nonce, &sign(&other, &a.pc_id(), "ph", &nonce)), Err(AuthErr::BadToken));
    }

    #[test]
    fn malformed_signatures_and_unknown_devices_are_refused() {
        let (a, _) = with_v2_phone();
        for bad in ["", "not base64!!", "AAAA", &URL_SAFE_NO_PAD.encode([0u8; 64])] {
            assert_eq!(a.verify_signature("ph", &[1u8; 32], bad), Err(AuthErr::BadToken), "{bad:?}");
        }
        assert_eq!(a.verify_signature("nobody", &[1u8; 32], "AAAA"), Err(AuthErr::Revoked));
        assert!(a.revoke("ph"));
        let (sk, _) = keypair(7);
        assert_eq!(a.verify_signature("ph", &[1u8; 32], &sign(&sk, &a.pc_id(), "ph", &[1u8; 32])), Err(AuthErr::Revoked));
    }

    #[test]
    fn a_key_based_phone_cannot_log_in_with_an_empty_token() {
        let (a, _) = with_v2_phone();
        assert_eq!(a.check_device("ph", ""), Err(AuthErr::BadToken));
    }

    #[test]
    fn invalid_public_keys_are_rejected() {
        let a = auth(false);
        let code = a.code();
        for bad in ["", "short", "!!!", &URL_SAFE_NO_PAD.encode([1u8; 31]), &URL_SAFE_NO_PAD.encode([1u8; 33])] {
            assert_eq!(a.request_pairing_v2(&code, "p", "x", "ip", bad, "web"), Err(PairErr::BadKey), "{bad:?}");
        }
        assert!(a.pending().is_empty());
    }

    #[test]
    fn rescanning_with_the_same_key_is_silent_but_a_new_key_needs_approval() {
        let (a, _) = with_v2_phone();
        let code = a.code();
        let (_, same) = keypair(7);
        assert_eq!(a.request_pairing_v2(&code, "ph", "Renamed", "ip", &same, "android"), Ok(Some(String::new())));
        assert!(a.pending().is_empty());
        // Another device claiming the same id must not take it over without the owner's OK.
        let (_, other) = keypair(8);
        assert_eq!(a.request_pairing_v2(&code, "ph", "Impostor", "ip", &other, "android"), Ok(None));
        assert_eq!(a.pending().len(), 1);
        let (_, pk_now) = { let d = a.devices().into_iter().next().unwrap().0; (d.id.clone(), d.pubkey) };
        assert_eq!(pk_now, same, "the stored key must not change before approval");
    }

    #[test]
    fn a_v1_phone_cannot_hijack_a_v2_device_id() {
        let (a, _) = with_v2_phone();
        let code = a.code();
        assert_eq!(a.request_pairing(&code, "ph", "Old app", "ip"), Ok(None), "needs the owner's approval, no silent token");
    }

    #[test]
    fn the_files_address_is_validated_stored_and_clearable() {
        let a = auth(false);
        assert!(a.set_files_url("  Files.Example.com "));
        assert_eq!(a.files_url().as_deref(), Some("https://files.example.com"));
        assert!(!a.set_files_url("http://insecure.example.com"), "plain http is refused");
        assert!(!a.set_files_url("files.example.com; calc"), "shell characters are refused");
        assert_eq!(a.files_url().as_deref(), Some("https://files.example.com"), "a refused address changes nothing");
        assert!(a.set_files_url(""));
        assert_eq!(a.files_url(), option_env!("PHONE_REMOTE_FILES_URL").and_then(crate::config::clean_files_url));
    }

    // ---- account certificates ----

    mod account {
        use super::*;
        use crate::account::{AccountTrust, Revoked};
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use std::collections::BTreeMap;

        const ACCT: &str = "abababababababababababababababab";

        fn service() -> SigningKey {
            SigningKey::from_bytes(&[5u8; 32])
        }

        fn joined() -> Auth {
            let a = auth(false);
            a.set_account(Some(AccountTrust {
                url: "https://x.test/account".into(),
                acct: ACCT.into(),
                keys: BTreeMap::from([("k1".to_string(), URL_SAFE_NO_PAD.encode(service().verifying_key().to_bytes()))]),
                grace_days: 14,
                input_allowed: true,
                revoked: vec![],
                revoked_checked: 0,
            }));
            a
        }

        fn cert(device: &str, kind: &str, phone_pk: &str, iat: u64, exp: u64) -> String {
            let body = serde_json::to_vec(&serde_json::json!({"v":1,"kid":"k1","acct":ACCT,"dev":device,"kind":kind,"name":"Pixel","pk":phone_pk,"iat":iat,"exp":exp})).unwrap();
            let mut msg = b"PRv2-cert\0".to_vec();
            msg.extend_from_slice(&body);
            format!("{}.{}", URL_SAFE_NO_PAD.encode(&body), URL_SAFE_NO_PAD.encode(service().sign(&msg).to_bytes()))
        }

        fn fresh(device: &str, pk: &str) -> String {
            let n = now_s();
            cert(device, "phone", pk, n - 60, n + 86_400)
        }

        #[test]
        fn an_account_phone_logs_in_with_its_certificate_and_no_pairing() {
            let a = joined();
            let (sk, pk) = keypair(3);
            let nonce = [1u8; 32];
            let sig = sign(&sk, &a.pc_id(), "phone-9", &nonce);
            assert_eq!(a.verify_account_login("phone-9", &fresh("phone-9", &pk), &nonce, &sig), Ok("Pixel".into()));
            assert!(a.input_allowed("phone-9"), "the owner's own phones may use the mouse and keyboard");
            assert!(a.devices().is_empty(), "nothing is added to the local trust list");
        }

        #[test]
        fn the_certificate_alone_is_not_enough() {
            let a = joined();
            let (_, pk) = keypair(3);
            let (thief, _) = keypair(4);
            let nonce = [1u8; 32];
            let c = fresh("phone-9", &pk);
            // someone who copied the certificate but not the key
            let sig = sign(&thief, &a.pc_id(), "phone-9", &nonce);
            assert_eq!(a.verify_account_login("phone-9", &c, &nonce, &sig), Err(AuthErr::BadToken));
            // a signature for another nonce or another PC
            let (sk, _) = keypair(3);
            assert_eq!(a.verify_account_login("phone-9", &c, &nonce, &sign(&sk, &a.pc_id(), "phone-9", &[2u8; 32])), Err(AuthErr::BadToken));
            assert_eq!(a.verify_account_login("phone-9", &c, &nonce, &sign(&sk, "other-pc", "phone-9", &nonce)), Err(AuthErr::BadToken));
            assert!(!a.input_allowed("phone-9"));
        }

        #[test]
        fn the_certificate_must_be_for_this_device_and_a_phone() {
            let a = joined();
            let (sk, pk) = keypair(3);
            let nonce = [1u8; 32];
            let sig = sign(&sk, &a.pc_id(), "phone-9", &nonce);
            let n = now_s();
            assert_eq!(a.verify_account_login("phone-9", &fresh("phone-1", &pk), &nonce, &sig), Err(AuthErr::BadToken));
            assert_eq!(a.verify_account_login("phone-9", &cert("phone-9", "pc", &pk, n - 60, n + 1000), &nonce, &sig), Err(AuthErr::BadToken));
        }

        #[test]
        fn expired_beyond_the_grace_period_is_refused_but_a_short_absence_is_fine() {
            let a = joined();
            let (sk, pk) = keypair(3);
            let nonce = [1u8; 32];
            let sig = sign(&sk, &a.pc_id(), "phone-9", &nonce);
            let n = now_s();
            assert!(a.verify_account_login("phone-9", &cert("phone-9", "phone", &pk, n - 40 * 86_400, n - 5 * 86_400), &nonce, &sig).is_ok());
            assert_eq!(a.verify_account_login("phone-9", &cert("phone-9", "phone", &pk, n - 90 * 86_400, n - 40 * 86_400), &nonce, &sig), Err(AuthErr::BadToken));
        }

        #[test]
        fn a_removed_device_is_told_to_forget_this_pc_and_cannot_log_in() {
            let a = joined();
            let (sk, pk) = keypair(3);
            let nonce = [1u8; 32];
            let sig = sign(&sk, &a.pc_id(), "phone-9", &nonce);
            let c = fresh("phone-9", &pk);
            assert!(a.verify_account_login("phone-9", &c, &nonce, &sig).is_ok());
            // the owner removes it; the PC next fetches the list
            let mut t = a.account().unwrap();
            t.revoked.push(Revoked { dev: "phone-9".into(), at: now_s() });
            a.set_account(Some(t));
            assert_eq!(a.verify_account_login("phone-9", &c, &nonce, &sig), Err(AuthErr::Revoked));
        }

        #[test]
        fn without_an_account_or_with_a_bad_certificate_a_locally_paired_phone_still_works() {
            let (sk, pk) = keypair(3);
            let nonce = [1u8; 32];
            // not joined to any account: a certificate is ignored, a stranger is refused
            let a = auth(false);
            let sig = sign(&sk, &a.pc_id(), "phone-9", &nonce);
            assert_eq!(a.verify_account_login("phone-9", &fresh("phone-9", &pk), &nonce, &sig), Err(AuthErr::BadToken));
            // paired locally as well: local key wins even when the certificate is junk
            let a = joined();
            let code = a.code();
            a.request_pairing_v2(&code, "phone-9", "Pixel", "ip", &pk, "android").unwrap();
            a.decide("phone-9", true);
            assert_eq!(a.verify_account_login("phone-9", "junk", &nonce, &sig), Ok("Pixel".into()));
        }

        #[test]
        fn leaving_the_account_forgets_account_phones() {
            let a = joined();
            let (sk, pk) = keypair(3);
            let nonce = [1u8; 32];
            let sig = sign(&sk, &a.pc_id(), "phone-9", &nonce);
            a.verify_account_login("phone-9", &fresh("phone-9", &pk), &nonce, &sig).unwrap();
            assert!(a.input_allowed("phone-9"));
            a.set_account(None);
            assert!(!a.input_allowed("phone-9"));
        }
    }
}

