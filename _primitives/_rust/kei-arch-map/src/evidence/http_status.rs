use std::net::IpAddr;
use std::time::Duration;
use url::{Host, Url};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(8);

/// Reject loopback / private / link-local / unspecified / multicast literal IPs.
fn ip_is_safe(ip: &IpAddr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    match ip {
        IpAddr::V4(v4) => !v4.is_private() && !v4.is_link_local() && !v4.is_broadcast(),
        IpAddr::V6(v6) => {
            // Stable Rust lacks v6.is_unique_local()/is_unicast_link_local();
            // approximate via prefix bits. ULA = fc00::/7, link-local = fe80::/10.
            let segs = v6.segments();
            let is_ula = (segs[0] & 0xfe00) == 0xfc00;
            let is_link_local = (segs[0] & 0xffc0) == 0xfe80;
            !is_ula && !is_link_local
        }
    }
}

/// Validate URL scheme + host. Hostnames pass through (egress firewall layer).
pub(super) fn validate_url(raw: &str) -> Result<Url, String> {
    let u = Url::parse(raw).map_err(|e| format!("invalid url `{}`: {}", raw, e))?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return Err(format!("scheme `{}` not allowed (http/https only)", u.scheme()));
    }
    match u.host() {
        Some(Host::Ipv4(v4)) => {
            if !ip_is_safe(&IpAddr::V4(v4)) {
                return Err(format!("ipv4 {} blocked (private/loopback/link-local)", v4));
            }
        }
        Some(Host::Ipv6(v6)) => {
            if !ip_is_safe(&IpAddr::V6(v6)) {
                return Err(format!("ipv6 {} blocked (ula/link-local/loopback)", v6));
            }
        }
        Some(Host::Domain(_)) => {}
        None => return Err("url missing host".to_string()),
    }
    Ok(u)
}

fn build_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TOTAL_TIMEOUT)
        .build()
        .map_err(|e| format!("http client build: {}", e))
}

pub fn check(url: &str, expected: &[u16]) -> (bool, String) {
    let parsed = match validate_url(url) {
        Ok(u) => u,
        Err(e) => return (false, e),
    };
    let client = match build_client() {
        Ok(c) => c,
        Err(e) => return (false, e),
    };
    let resp = match client.get(parsed.as_str()).send() {
        Ok(r) => r,
        Err(e) => return (false, format!("GET {} failed: {}", url, e)),
    };
    let status = resp.status().as_u16();
    if expected.contains(&status) {
        (true, String::new())
    } else {
        (false, format!("status {} not in {:?}", status, expected))
    }
}
