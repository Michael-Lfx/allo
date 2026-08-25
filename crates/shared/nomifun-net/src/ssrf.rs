//! Canonical SSRF address policy for every outbound fetch path.
//!
//! One list, one meaning: an address is blocked when it can plausibly reach a
//! host on the operator's own networks — loopback, RFC1918, link-local
//! (including the `169.254.169.254` cloud metadata endpoint), CGNAT, IPv6 ULA
//! and link-local, documentation ranges, and the IPv4 forms embedded in IPv6.
//!
//! Two special-purpose IPv4 ranges are deliberately NOT blocked:
//!
//! - `198.18.0.0/15` (RFC 2544 benchmarking)
//! - `240.0.0.0/4` (RFC 1112 reserved)
//!
//! TUN-mode VPN clients (mihomo/Clash, sing-box, Surge, tun2socks) answer DNS
//! with synthetic "fake-IP" addresses from exactly those ranges and route them
//! through the tunnel, so a hostname like `api.github.com` resolves to
//! `198.18.0.24` on a machine behind such a tunnel. Neither range carries real
//! internet traffic nor hosts internal services, so treating them as internal
//! buys no protection while blocking every fetch the product makes.
//!
//! [`ALLOW_CIDRS_ENV`] adds CIDRs for tunnels that map into other space.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::LazyLock;

/// Comma-separated CIDRs to exempt from the block list, for tunnels that map
/// DNS answers into space this policy otherwise treats as internal. Read once
/// per process.
pub const ALLOW_CIDRS_ENV: &str = "NOMIFUN_SSRF_ALLOW_CIDRS";

static ALLOWED_CIDRS: LazyLock<Vec<Cidr>> = LazyLock::new(|| {
    std::env::var(ALLOW_CIDRS_ENV)
        .map(|value| parse_cidr_list(&value))
        .unwrap_or_default()
});

/// `true` when `ip` must not be used as an outbound target.
pub fn is_blocked_target(ip: &IpAddr) -> bool {
    !is_operator_allowed(ip) && is_internal_address(ip)
}

/// Raw classification, ignoring [`ALLOW_CIDRS_ENV`].
pub fn is_internal_address(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_internal_v4(v4),
        IpAddr::V6(v6) => is_internal_v6(v6),
    }
}

fn is_internal_v4(v4: &Ipv4Addr) -> bool {
    let octets = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_multicast()
        || v4.is_documentation()
        || octets[0] == 0
        // CGNAT 100.64.0.0/10.
        || (octets[0] == 100 && (64..128).contains(&octets[1]))
        // IETF protocol assignments 192.0.0.0/24.
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
}

fn is_internal_v6(v6: &Ipv6Addr) -> bool {
    let segments = v6.segments();
    let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    let benchmarking = segments[0] == 0x2001 && segments[1] == 0x0002;
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()
        || documentation
        || benchmarking
        // Unique-local fc00::/7.
        || (segments[0] & 0xfe00) == 0xfc00
        // Link-local fe80::/10.
        || (segments[0] & 0xffc0) == 0xfe80
        || embedded_ipv4(v6).is_some_and(|v4| is_internal_v4(&v4))
}

/// IPv4-mapped (`::ffff:a.b.c.d`) and IPv4-compatible (`::a.b.c.d`) forms, so
/// an IPv6 literal cannot smuggle a blocked IPv4 target past the policy.
fn embedded_ipv4(v6: &Ipv6Addr) -> Option<Ipv4Addr> {
    v6.to_ipv4_mapped().or_else(|| v6.to_ipv4())
}

fn is_operator_allowed(ip: &IpAddr) -> bool {
    if ALLOWED_CIDRS.is_empty() {
        return false;
    }
    let embedded = match ip {
        IpAddr::V6(v6) => embedded_ipv4(v6).map(IpAddr::V4),
        IpAddr::V4(_) => None,
    };
    ALLOWED_CIDRS.iter().any(|cidr| {
        cidr.contains(ip) || embedded.as_ref().is_some_and(|inner| cidr.contains(inner))
    })
}

fn parse_cidr_list(value: &str) -> Vec<Cidr> {
    value.split(',').filter_map(Cidr::parse).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cidr {
    base: IpAddr,
    prefix: u8,
}

impl Cidr {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        let (address, prefix) = match value.split_once('/') {
            Some((address, prefix)) => (address.trim(), Some(prefix.trim().parse::<u8>().ok()?)),
            None => (value, None),
        };
        let base: IpAddr = address.parse().ok()?;
        let full = if base.is_ipv4() { 32 } else { 128 };
        let prefix = prefix.unwrap_or(full);
        (prefix <= full).then_some(Self { base, prefix })
    }

    fn contains(&self, ip: &IpAddr) -> bool {
        match (self.base, ip) {
            (IpAddr::V4(base), IpAddr::V4(ip)) => {
                masked_v4(base, self.prefix) == masked_v4(*ip, self.prefix)
            }
            (IpAddr::V6(base), IpAddr::V6(ip)) => {
                masked_v6(base, self.prefix) == masked_v6(*ip, self.prefix)
            }
            _ => false,
        }
    }
}

fn masked_v4(ip: Ipv4Addr, prefix: u8) -> u32 {
    let bits = u32::from(ip);
    if prefix == 0 {
        0
    } else {
        bits & (u32::MAX << (32 - prefix))
    }
}

fn masked_v6(ip: Ipv6Addr, prefix: u8) -> u128 {
    let bits = u128::from(ip);
    if prefix == 0 {
        0
    } else {
        bits & (u128::MAX << (128 - prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(value: &str) -> IpAddr {
        value.parse().expect("test address")
    }

    #[test]
    fn blocks_internal_and_metadata_targets() {
        for value in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.0.0.1",
            "192.0.2.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fd00::1",
            "fe80::1",
            "2001:db8::1",
            "2001:2::1",
            "::ffff:127.0.0.1",
            "::192.168.0.1",
        ] {
            assert!(
                is_blocked_target(&ip(value)),
                "{value} must stay blocked"
            );
        }
    }

    #[test]
    fn allows_tunnel_fake_ip_and_public_targets() {
        // 198.18.0.0/15 and 240.0.0.0/4 are the fake-IP space of TUN-mode VPN
        // clients: blocking them breaks every fetch behind such a tunnel.
        for value in [
            "198.18.0.24",
            "198.19.255.255",
            "240.0.0.1",
            "1.1.1.1",
            "140.82.121.6",
            "2606:4700::1111",
        ] {
            assert!(
                !is_blocked_target(&ip(value)),
                "{value} must remain reachable"
            );
        }
    }

    #[test]
    fn cidr_parsing_and_matching() {
        let cidr = Cidr::parse("198.18.0.0/15").expect("cidr");
        assert!(cidr.contains(&ip("198.18.0.24")));
        assert!(cidr.contains(&ip("198.19.0.1")));
        assert!(!cidr.contains(&ip("198.20.0.1")));
        assert!(!cidr.contains(&ip("fc00::1")));

        let host = Cidr::parse("10.1.2.3").expect("bare host cidr");
        assert_eq!(host.prefix, 32);
        assert!(host.contains(&ip("10.1.2.3")));
        assert!(!host.contains(&ip("10.1.2.4")));

        let v6 = Cidr::parse("fc00::/18").expect("v6 cidr");
        assert!(v6.contains(&ip("fc00::1")));
        assert!(!v6.contains(&ip("fd00::1")));

        assert_eq!(Cidr::parse("not-an-ip/8"), None);
        assert_eq!(Cidr::parse("10.0.0.0/33"), None);
        assert_eq!(Cidr::parse("  "), None);
    }

    #[test]
    fn allow_list_parsing_skips_invalid_entries() {
        let parsed = parse_cidr_list("198.18.0.0/15, garbage, fc00::/18,");
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].contains(&ip("198.18.5.5")));
        assert!(parsed[1].contains(&ip("fc00::9")));
    }
}
