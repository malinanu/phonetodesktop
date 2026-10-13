//! Talking to the optional account service: join an account with a link code, and fetch the signed list of
//! removed devices. Blocking (ureq); callers run it off the async runtime.
//! The data path never goes through the service: it is only asked for keys, a certificate and a revocation list.

use crate::account::AccountTrust;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::SigningKey;
use rand::RngCore;
use std::collections::BTreeMap;
use std::time::Duration;

/// How often a joined computer asks for the revocation list.
pub const POLL_EVERY: Duration = Duration::from_secs(15 * 60);

/// The address of the account service, cleaned up: `https://` required (plain `http://` only for this machine,
/// for development), no query, fragment or trailing slash. None if it is not usable.
pub fn normalize_base(input: &str) -> Option<String> {
    let t = input.trim();
    if t.is_empty() || t.len() > 200 || t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (scheme, rest) = match t.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => ("https".to_string(), t),
    };
    if rest.contains(['?', '#', '@', '\\']) {
        return None;
    }
    let rest = rest.trim_end_matches('/');
    let (host_port, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, String::new()),
    };
    let host = host_port.rsplit_once(':').map_or(host_port, |(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host_port });
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return None;
    }
    let local = matches!(host, "localhost" | "127.0.0.1");
    match scheme.as_str() {
        "https" => {}
        "http" if local => {}
        _ => return None,
    }
    Some(format!("{scheme}://{host_port}{path}"))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).user_agent("phone-remote-agent").build()
}

fn explain(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let detail = resp.into_json::<serde_json::Value>().ok().and_then(|v| v["detail"].as_str().map(str::to_owned));
            detail.unwrap_or_else(|| format!("The account service answered {code}."))
        }
        ureq::Error::Transport(_) => "Could not reach the account service. Check the address and your internet connection.".to_string(),
    }
}

/// Join the account that made `code` (Account page, "Add a computer"). Learns the service's keys, enrols this
/// computer as a device (with the id and TLS fingerprint phones use to find and check it) and checks the certificate it gets back.
pub fn join(base: &str, code: &str, host_name: &str, pc_id: &str, tls_fp: Option<&str>, now: u64) -> Result<AccountTrust, String> {
    let base = normalize_base(base).ok_or("That address does not look right. It must start with https://")?;
    let a = agent();

    let mut id = [0u8; 16];
    rand::rng().fill_bytes(&mut id);
    let mut seed = [0u8; 32];
    rand::rng().fill_bytes(&mut seed); // only the public half is sent and the seed is dropped with this function
    let device_id: String = id.iter().map(|b| format!("{b:02x}")).collect();
    let pubkey = URL_SAFE_NO_PAD.encode(SigningKey::from_bytes(&seed).verifying_key().to_bytes());
    let platform = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    };
    let name: String = host_name.chars().filter(|c| !c.is_control()).take(60).collect();
    let name = if name.trim().is_empty() { "Computer".to_string() } else { name };

    let reply: serde_json::Value = a
        .post(&format!("{base}/v1/links/claim"))
        .send_json(serde_json::json!({"code": code.trim(), "id": device_id, "name": name, "platform": platform, "kind": "pc", "pubkey": pubkey, "pc_id": pc_id, "fingerprint": tls_fp}))
        .map_err(explain)?
        .into_json()
        .map_err(|_| "The account service gave an answer this program does not understand.")?;

    let acct = reply["account_id"].as_str().ok_or("The account service gave an answer this program does not understand.")?.to_owned();
    let mut keys = BTreeMap::new();
    for k in reply["keys"]["keys"].as_array().ok_or("The account service sent no keys.")? {
        if let (Some(kid), Some(pk)) = (k["kid"].as_str(), k["pk"].as_str()) {
            keys.insert(kid.to_owned(), pk.to_owned());
        }
    }
    let trust = AccountTrust { url: base, acct, keys, device_id, ..Default::default() };
    // The certificate is checked against the keys we just learned; this catches a broken or mismatched service.
    trust.verify_cert(reply["cert"].as_str().unwrap_or(""), now).map_err(|e| format!("The account service sent a certificate that does not check out ({e:?})."))?;
    Ok(trust)
}

/// The service's current signed list of removed devices for this account (verified later, by `apply_revocations`).
pub fn fetch_revocations(trust: &AccountTrust) -> Result<String, String> {
    let url = format!("{}/v1/accounts/{}/revocations", trust.url, trust.acct);
    agent().get(&url).call().map_err(explain)?.into_string().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_cleaned_and_unsafe_ones_refused() {
        assert_eq!(normalize_base("https://files.example.com/account/").as_deref(), Some("https://files.example.com/account"));
        assert_eq!(normalize_base("  files.example.com  ").as_deref(), Some("https://files.example.com"));
        assert_eq!(normalize_base("HTTPS://Files.Example.com:8443").as_deref(), Some("https://Files.Example.com:8443"));
        assert_eq!(normalize_base("http://127.0.0.1:8000").as_deref(), Some("http://127.0.0.1:8000"));
        assert_eq!(normalize_base("http://localhost/account").as_deref(), Some("http://localhost/account"));
        for bad in ["", "http://example.com", "ftp://example.com", "https://", "https://a b.com", "https://user@example.com", "https://example.com?x=1", "https://exa_mple.com", "javascript:alert(1)", "https://example.com#x"] {
            assert_eq!(normalize_base(bad), None, "{bad}");
        }
    }
}
