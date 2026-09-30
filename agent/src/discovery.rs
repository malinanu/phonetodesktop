//! Advertise the agent over mDNS so the Android app can find it after an IP change.

use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceInfo};

pub const SERVICE: &str = "_phoneremote._tcp.local.";

pub fn advertise(host: &str, port: u16) -> Result<ServiceDaemon> {
    let daemon = ServiceDaemon::new()?;
    let safe: String = host.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let hostname = format!("{safe}.local.");
    // `enable_addr_auto` publishes every interface address and tracks changes.
    let info = ServiceInfo::new(SERVICE, host, &hostname, "", port, &[("v", "1")][..])?.enable_addr_auto();
    daemon.register(info)?;
    Ok(daemon)
}
