//! HTTPS for the phone connection, without any certificate authority.
//!
//! Each computer makes one self-signed certificate (ECDSA P-256) the first time it runs and keeps it, with its
//! private key, in its own folder. The pairing QR carries the SHA-256 of the certificate's public key
//! (SubjectPublicKeyInfo, base64url); the phone trusts exactly that key and nothing else, so nobody on the same
//! Wi-Fi can read or change the traffic. The same listener also still speaks plain HTTP (for the browser
//! dashboard on this computer, and for older phones while the owner allows them): it looks at the first byte of
//! each connection to tell the two apart.

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskCtx, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::server::TlsStream;
use tokio_rustls::{rustls, TlsAcceptor};

const CERT_FILE: &str = "tls-cert.der";
const KEY_FILE: &str = "tls-key.der";
const META_FILE: &str = "tls-meta.json";
const VALID_DAYS: i64 = 3650;
const RENEW_BEFORE_S: u64 = 60 * 86_400;

pub struct Identity {
    pub cert_der: Vec<u8>,
    /// PKCS#8 private key.
    pub key_der: Vec<u8>,
    /// What the QR carries: base64url (no padding) SHA-256 of the certificate's SubjectPublicKeyInfo.
    pub fingerprint: String,
}

pub fn fingerprint_of_spki(spki: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(spki))
}

// ---- reading the public key out of a certificate (minimal DER walk) --------------------------------------

/// One DER element at the start of `b`: (tag, header length, content length).
fn der_element(b: &[u8]) -> Option<(u8, usize, usize)> {
    let tag = *b.first()?;
    let l0 = *b.get(1)? as usize;
    if l0 < 0x80 {
        return Some((tag, 2, l0));
    }
    let n = l0 & 0x7f;
    if n == 0 || n > 4 || b.len() < 2 + n {
        return None;
    }
    let len = b[2..2 + n].iter().fold(0usize, |a, x| (a << 8) | *x as usize);
    Some((tag, 2 + n, len))
}

/// The SubjectPublicKeyInfo (complete DER SEQUENCE) of an X.509 certificate.
pub fn spki_from_cert(cert: &[u8]) -> Option<&[u8]> {
    let (t, h, l) = der_element(cert)?; // Certificate ::= SEQUENCE
    if t != 0x30 || h + l > cert.len() {
        return None;
    }
    let (t, h2, l2) = der_element(&cert[h..])?; // tbsCertificate ::= SEQUENCE
    if t != 0x30 || h2 + l2 > cert.len() - h {
        return None;
    }
    let mut rest = &cert[h + h2..h + h2 + l2];
    let skip = |rest: &mut &[u8]| -> Option<(u8, usize)> {
        let (t, h, l) = der_element(rest)?;
        if h + l > rest.len() {
            return None;
        }
        let whole = h + l;
        let r = &rest[..whole];
        *rest = &rest[whole..];
        Some((t, r.len()))
    };
    // version [0] is optional, then serial, signature, issuer, validity, subject
    let (first_tag, _) = {
        let (t, _, _) = der_element(rest)?;
        (t, ())
    };
    if first_tag == 0xA0 {
        skip(&mut rest)?;
    }
    for _ in 0..5 {
        skip(&mut rest)?;
    }
    let (t, h3, l3) = der_element(rest)?; // subjectPublicKeyInfo ::= SEQUENCE
    if t != 0x30 || h3 + l3 > rest.len() {
        return None;
    }
    Some(&rest[..h3 + l3])
}

// ---- making and keeping the identity ------------------------------------------------------------------------

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> Result<()> {
    let tmp = path.with_extension("tmp");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(if private { 0o600 } else { 0o644 }).open(&tmp)?;
        f.write_all(bytes)?;
    }
    #[cfg(not(unix))]
    {
        let _ = private; // the folder is inside the user's profile, which Windows already restricts to them
        std::fs::write(&tmp, bytes)?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// DNS names that are safe to put in the certificate: letters, digits, dashes and dots only.
fn dns_name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 253 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

fn make_cert(key: &rcgen::KeyPair, hostname: &str) -> Result<(Vec<u8>, u64)> {
    let mut names = vec!["localhost".to_string()];
    if dns_name_ok(hostname) && hostname != "localhost" {
        names.push(hostname.to_string());
    }
    let mut params = rcgen::CertificateParams::new(names).map_err(|e| anyhow!("certificate names: {e}"))?;
    params.subject_alt_names.push(rcgen::SanType::IpAddress(IpAddr::from([127, 0, 0, 1])));
    params.subject_alt_names.push(rcgen::SanType::IpAddress(IpAddr::from([0u16, 0, 0, 0, 0, 0, 0, 1])));
    params.distinguished_name.push(rcgen::DnType::CommonName, "Phone Remote");
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1); // tolerate phones with a slightly wrong clock
    params.not_after = now + time::Duration::days(VALID_DAYS);
    let not_after = params.not_after.unix_timestamp().max(0) as u64;
    let cert = params.self_signed(key).map_err(|e| anyhow!("signing the certificate: {e}"))?;
    Ok((cert.der().to_vec(), not_after))
}

/// Load this computer's identity from `dir`, creating it the first time. The key never changes, so the
/// fingerprint in already-scanned QR codes stays valid; only the certificate is renewed (long before it expires).
pub fn load_or_create(dir: &Path, hostname: &str) -> Result<Identity> {
    std::fs::create_dir_all(dir)?;
    let key_path = dir.join(KEY_FILE);
    let key = match std::fs::read(&key_path).ok().and_then(|der| rcgen::KeyPair::try_from(der.as_slice()).ok()) {
        Some(k) => k,
        None => {
            let k = rcgen::KeyPair::generate().map_err(|e| anyhow!("generating the key: {e}"))?;
            write_atomic(&key_path, &k.serialize_der(), true).context("saving the TLS key")?;
            let _ = std::fs::remove_file(dir.join(CERT_FILE)); // a certificate for another key is useless
            k
        }
    };
    let cert_path = dir.join(CERT_FILE);
    let not_after: u64 = std::fs::read_to_string(dir.join(META_FILE))
        .ok()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok())
        .and_then(|v| v["not_after"].as_u64())
        .unwrap_or(0);
    let cert_der = match std::fs::read(&cert_path) {
        Ok(c) if not_after > now_s() + RENEW_BEFORE_S && spki_from_cert(&c) == Some(key.public_key_der().as_slice()) => c,
        _ => {
            let (der, na) = make_cert(&key, hostname)?;
            write_atomic(&cert_path, &der, false).context("saving the TLS certificate")?;
            write_atomic(&dir.join(META_FILE), serde_json::json!({ "not_after": na }).to_string().as_bytes(), false)?;
            der
        }
    };
    let spki = spki_from_cert(&cert_der).ok_or_else(|| anyhow!("the certificate has no readable public key"))?;
    Ok(Identity { fingerprint: fingerprint_of_spki(spki), cert_der, key_der: key.serialize_der() })
}

pub fn acceptor(id: &Identity) -> Result<TlsAcceptor> {
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(id.key_der.clone()));
    let cert = rustls::pki_types::CertificateDer::from(id.cert_der.clone());
    let mut cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| anyhow!("TLS versions: {e}"))?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| anyhow!("TLS certificate: {e}"))?;
    // WebSocket needs HTTP/1.1; do not offer h2.
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

// ---- one port, two protocols --------------------------------------------------------------------------------

/// A connection that is either plain TCP or TLS.
pub enum MaybeTls {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for MaybeTls {
    fn poll_read(self: Pin<&mut Self>, cx: &mut TaskCtx<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTls::Plain(s) => Pin::new(s).poll_read(cx, buf),
            MaybeTls::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for MaybeTls {
    fn poll_write(self: Pin<&mut Self>, cx: &mut TaskCtx<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            MaybeTls::Plain(s) => Pin::new(s).poll_write(cx, buf),
            MaybeTls::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskCtx<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTls::Plain(s) => Pin::new(s).poll_flush(cx),
            MaybeTls::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskCtx<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTls::Plain(s) => Pin::new(s).poll_shutdown(cx),
            MaybeTls::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum PlainAction {
    Serve,
    /// Plain HTTP from another machine while older phones are switched off: answer with a notice and close.
    Refuse,
}

/// Plain HTTP is always fine from this computer itself (the dashboard); from the network only while the
/// owner still allows older phones.
pub fn plain_action(peer: IpAddr, older_phones_allowed: bool) -> PlainAction {
    if peer.is_loopback() || older_phones_allowed {
        PlainAction::Serve
    } else {
        PlainAction::Refuse
    }
}

const REFUSED_NOTICE: &str = "HTTP/1.0 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nCache-Control: no-store\r\n\r\nThis computer only accepts the secure Phone Remote app. Install or update the Phone Remote app on your phone and scan the code again.\n";

/// Accepts TCP connections, tells TLS from plain HTTP by the first byte, and hands finished connections to
/// axum. The sniffing and the TLS handshake happen in their own tasks, so one slow or silent client can never
/// hold up the others.
pub struct SniffListener {
    rx: mpsc::Receiver<(MaybeTls, SocketAddr)>,
    local: SocketAddr,
}

impl SniffListener {
    pub fn new(listener: TcpListener, acceptor: TlsAcceptor, older_phones_allowed: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<Self> {
        let local = listener.local_addr()?;
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            loop {
                let (stream, peer) = match listener.accept().await {
                    Ok(c) => c,
                    Err(_) => {
                        // e.g. out of file descriptors: back off instead of spinning
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let (tx, acceptor, allowed) = (tx.clone(), acceptor.clone(), older_phones_allowed.clone());
                tokio::spawn(async move {
                    let _ = stream.set_nodelay(true);
                    let mut first = [0u8; 1];
                    match tokio::time::timeout(Duration::from_secs(5), stream.peek(&mut first)).await {
                        Ok(Ok(1)) => {}
                        _ => return, // closed, errored or said nothing for 5 s
                    }
                    if first[0] == 0x16 {
                        // TLS handshake record
                        if let Ok(Ok(t)) = tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await {
                            let _ = tx.send((MaybeTls::Tls(Box::new(t)), peer)).await;
                        }
                    } else {
                        match plain_action(peer.ip(), allowed()) {
                            PlainAction::Serve => {
                                let _ = tx.send((MaybeTls::Plain(stream), peer)).await;
                            }
                            PlainAction::Refuse => {
                                let mut s = stream;
                                let _ = s.write_all(REFUSED_NOTICE.as_bytes()).await;
                                let _ = s.shutdown().await;
                            }
                        }
                    }
                });
            }
        });
        Ok(SniffListener { rx, local })
    }
}

impl axum::serve::Listener for SniffListener {
    type Io = MaybeTls;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.rx.recv().await {
            Some(c) => c,
            // The accept task runs for the life of the process; if it is ever gone there is nothing to wait for.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pr-tls-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn fingerprint_is_base64url_sha256() {
        // SHA-256 of the empty input
        assert_eq!(fingerprint_of_spki(b""), "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU");
    }

    #[test]
    fn the_public_key_is_read_back_out_of_the_certificate() {
        let d = tmp("spki");
        let id = load_or_create(&d, "my-pc").unwrap();
        let key = rcgen::KeyPair::try_from(id.key_der.as_slice()).unwrap();
        assert_eq!(spki_from_cert(&id.cert_der).unwrap(), key.public_key_der().as_slice());
        assert_eq!(id.fingerprint, fingerprint_of_spki(&key.public_key_der()));
        assert_eq!(spki_from_cert(&[]), None);
        assert_eq!(spki_from_cert(&[0x30, 0x03, 1, 2, 3]), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn identity_is_created_once_and_then_reused() {
        let d = tmp("reuse");
        let a = load_or_create(&d, "my-pc").unwrap();
        let b = load_or_create(&d, "my-pc").unwrap();
        assert_eq!((&a.fingerprint, &a.cert_der, &a.key_der), (&b.fingerprint, &b.cert_der, &b.key_der));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(d.join(KEY_FILE)).unwrap().permissions().mode() & 0o777, 0o600, "the private key must be owner-only");
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn an_expiring_certificate_is_renewed_but_the_fingerprint_stays() {
        let d = tmp("renew");
        let a = load_or_create(&d, "my-pc").unwrap();
        std::fs::write(d.join(META_FILE), r#"{"not_after":1}"#).unwrap();
        let b = load_or_create(&d, "my-pc").unwrap();
        assert_ne!(a.cert_der, b.cert_der, "a new certificate was issued");
        assert_eq!(a.fingerprint, b.fingerprint, "same key, so phones that pinned it still trust it");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_damaged_key_is_replaced_with_a_new_identity() {
        let d = tmp("damaged");
        let a = load_or_create(&d, "my-pc").unwrap();
        std::fs::write(d.join(KEY_FILE), b"garbage").unwrap();
        let b = load_or_create(&d, "my-pc").unwrap();
        assert_ne!(a.fingerprint, b.fingerprint);
        assert!(acceptor(&b).is_ok());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn odd_host_names_do_not_break_the_certificate() {
        let d = tmp("names");
        for name in ["", "UPPER-case", "with space", "ünïcode", "a_b", "localhost"] {
            assert!(load_or_create(&d, name).is_ok(), "{name:?}");
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn plain_http_policy() {
        let lan: IpAddr = "192.168.1.9".parse().unwrap();
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(plain_action(lo, false), PlainAction::Serve, "this computer's own dashboard always works");
        assert_eq!(plain_action(lan, true), PlainAction::Serve);
        assert_eq!(plain_action(lan, false), PlainAction::Refuse);
        assert_eq!(plain_action("::1".parse().unwrap(), false), PlainAction::Serve);
    }
}
