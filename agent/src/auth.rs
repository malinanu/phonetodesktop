//! Who may control this PC: approved phones with their own tokens, plus the legacy shared secret.
//! Pure logic (no sockets), so the pairing state machine is unit-tested.

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
}

struct PendingEntry {
    id: String,
    name: String,
    ip: String,
    at: Instant,
}

struct Inner {
    cfg: Config,
    code: String,
    code_at: Instant,
    pending: Vec<PendingEntry>,
    decisions: HashMap<String, Decision>,
    online: HashMap<String, usize>,
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

impl Auth {
    pub fn new(cfg: Config) -> Self {
        Self::build(cfg, true)
    }

    #[cfg(test)]
    fn in_memory(cfg: Config) -> Self {
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
        if !ct_eq(&d.token, token) {
            return Err(AuthErr::BadToken);
        }
        d.last_seen = now_s();
        Ok(d.name.clone())
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
        let mut i = self.inner.lock().unwrap();
        if !ct_eq(&i.code, code) {
            return Err(PairErr::BadCode);
        }
        if i.code_at.elapsed() > CODE_TTL {
            return Err(PairErr::Expired);
        }
        let name: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
        if let Some(d) = i.cfg.devices.iter_mut().find(|d| d.id == device) {
            // Known phone scanning again: no need to bother the owner.
            d.token = config::new_token();
            d.name = name;
            let t = d.token.clone();
            self.save(&i);
            return Ok(Some(t));
        }
        i.pending.retain(|p| p.id != device && p.at.elapsed() < PENDING_TTL);
        i.decisions.remove(device);
        i.pending.push(PendingEntry { id: device.into(), name, ip: ip.into(), at: Instant::now() });
        Ok(None)
    }

    pub fn pending(&self) -> Vec<Pending> {
        let mut i = self.inner.lock().unwrap();
        i.pending.retain(|p| p.at.elapsed() < PENDING_TTL);
        i.pending
            .iter()
            .map(|p| Pending { id: p.id.clone(), name: p.name.clone(), ip: p.ip.clone(), age_s: p.at.elapsed().as_secs() })
            .collect()
    }

    /// Owner's answer. Approving mints the phone's own token.
    pub fn decide(&self, device: &str, approve: bool) -> bool {
        let mut i = self.inner.lock().unwrap();
        let Some(pos) = i.pending.iter().position(|p| p.id == device) else { return false };
        let p = i.pending.remove(pos);
        if approve {
            let token = config::new_token();
            i.cfg.devices.retain(|d| d.id != p.id);
            i.cfg.devices.push(Device { id: p.id.clone(), name: p.name, token: token.clone(), created: now_s(), last_seen: now_s(), input_allowed: true });
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
        self.inner.lock().unwrap().cfg.devices.iter().find(|d| d.id == device).is_some_and(|d| d.input_allowed)
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
            port: 1,
            local_secret: String::new(),
            autostart_initialized: true,
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
}
