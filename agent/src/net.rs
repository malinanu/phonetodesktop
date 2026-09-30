//! LAN address discovery and the private-network guard.

use std::net::{IpAddr, Ipv4Addr};

/// RFC1918, link-local, loopback and CGNAT (Tailscale). Anything else is refused.
pub fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_lan(IpAddr::V4(v4));
            }
            v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

const VIRTUAL_HINTS: &[&str] = &["vethernet", "virtualbox", "vmware", "vmnet", "docker", "veth", "br-", "wsl", "loopback", "bluetooth"];

/// Candidate IPv4 addresses phones can reach, best guess first.
pub fn lan_addrs() -> Vec<Ipv4Addr> {
    let mut real = vec![];
    let mut virt = vec![];
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        let IpAddr::V4(v4) = i.ip() else { continue };
        if i.is_loopback() || !v4.is_private() {
            continue;
        }
        let n = i.name.to_lowercase();
        if VIRTUAL_HINTS.iter().any(|h| n.contains(h)) {
            virt.push(v4);
        } else {
            real.push(v4);
        }
    }
    real.extend(virt);
    real
}

pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()))
        .unwrap_or_else(|_| "desktop".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard() {
        assert!(is_lan("192.168.1.5".parse().unwrap()));
        assert!(is_lan("10.0.0.2".parse().unwrap()));
        assert!(is_lan("172.16.9.9".parse().unwrap()));
        assert!(is_lan("100.101.1.1".parse().unwrap()));
        assert!(!is_lan("8.8.8.8".parse().unwrap()));
        assert!(!is_lan("172.32.0.1".parse().unwrap()));
        assert!(!is_lan("100.128.0.1".parse().unwrap()));
        assert!(is_lan("::ffff:192.168.1.5".parse().unwrap()));
    }
}
