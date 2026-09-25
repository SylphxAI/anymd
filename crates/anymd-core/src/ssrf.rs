//! DNS resolution and globally-routable address policy for URL PDF loading.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};

pub(crate) trait DnsResolver: Send + Sync {
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SystemDnsResolver;

impl DnsResolver for SystemDnsResolver {
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        (host, port)
            .to_socket_addrs()
            .map(|addresses| addresses.collect())
    }
}

fn is_non_global_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 0
        || o[0] == 10
        || o[0] == 127
        || (o[0] == 169 && o[1] == 254)
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 192 && o[1] == 0 && o[2] == 2)
        || (o[0] == 192 && o[1] == 88 && o[2] == 99)
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 100 && (64..=127).contains(&o[1]))
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)
        || o[0] >= 224
}

fn hextets_to_ipv4(hi: u16, lo: u16) -> Ipv4Addr {
    Ipv4Addr::new(
        ((hi >> 8) & 0xff) as u8,
        (hi & 0xff) as u8,
        ((lo >> 8) & 0xff) as u8,
        (lo & 0xff) as u8,
    )
}

fn is_non_global_ipv6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    if ip.is_unspecified() || ip.is_loopback() {
        return true;
    }
    // ULA, deprecated site-local, link-local, and multicast.
    if (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfec0
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xff00) == 0xff00
    {
        return true;
    }
    // IPv4-mapped and IPv4-compatible forms inherit the IPv4 policy.
    if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0xffff {
        return is_non_global_ipv4(hextets_to_ipv4(s[6], s[7]));
    }
    if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0 {
        return is_non_global_ipv4(hextets_to_ipv4(s[6], s[7]));
    }
    // NAT64 well-known prefix embeds an IPv4 address.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0 {
        return is_non_global_ipv4(hextets_to_ipv4(s[6], s[7]));
    }
    // NAT64 local-use, discard-only, documentation, benchmarking, ORCHID,
    // and Teredo are not valid public fetch targets.
    if (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1)
        || (s[0] == 0x100 && s[1] == 0 && s[2] == 0 && s[3] == 0)
        || (s[0] == 0x2001 && s[1] == 0)
        || (s[0] == 0x2001 && s[1] == 2)
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        || (s[0] == 0x2001 && (s[1] & 0xfff0) == 0x0010)
        || (s[0] == 0x2001 && (s[1] & 0xfff0) == 0x0020)
    {
        return true;
    }
    // 6to4 inherits the policy of its embedded IPv4 address.
    if s[0] == 0x2002 {
        return is_non_global_ipv4(hextets_to_ipv4(s[1], s[2]));
    }
    false
}

/// Returns true for addresses that must never be reached by URL PDF loading.
///
/// The historical public name is retained for compatibility; the policy is
/// intentionally broader than RFC1918 and rejects non-global special-use space.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_non_global_ipv4(v4),
        IpAddr::V6(v6) => is_non_global_ipv6(v6),
    }
}

pub(crate) fn resolve_public_addrs_with<R, F>(
    host: &str,
    port: u16,
    resolver: &R,
    is_denied: F,
) -> Result<Vec<SocketAddr>, String>
where
    R: DnsResolver,
    F: Fn(IpAddr) -> bool,
{
    let host = host.trim().trim_matches(|c| c == '[' || c == ']');
    let mut addresses = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        resolver
            .resolve(host, port)
            .map_err(|_| format!("URL host '{host}' could not be resolved."))?
    };

    if addresses.is_empty() {
        return Err(format!("URL host '{host}' resolved to no addresses."));
    }
    if addresses.iter().any(|address| is_denied(address.ip())) {
        return Err(format!(
            "URL host '{host}' resolves to a non-public address (SSRF protection)."
        ));
    }
    if addresses.iter().any(|address| address.port() != port) {
        return Err(format!(
            "URL host '{host}' resolved with an unexpected port (SSRF protection)."
        ));
    }

    addresses.sort_unstable();
    addresses.dedup();
    Ok(addresses)
}

pub(crate) fn resolve_public_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    resolve_public_addrs_with(host, port, &SystemDnsResolver, is_private_ip)
}

pub fn assert_host_not_private(host: &str) -> Result<(), String> {
    resolve_public_addrs(host, 0).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FixedResolver(Vec<SocketAddr>);

    impl DnsResolver for FixedResolver {
        fn resolve(&self, _host: &str, _port: u16) -> io::Result<Vec<SocketAddr>> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn blocks_private_transition_and_special_use_addresses() {
        for value in [
            "127.0.0.1",
            "169.254.169.254",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "::1",
            "64:ff9b::a9fe:a9fe",
            "2001:db8::1",
            "2002:7f00:1::",
        ] {
            assert!(is_private_ip(value.parse().expect("test IP")), "{value}");
        }
        for value in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(!is_private_ip(value.parse().expect("test IP")), "{value}");
        }
    }

    #[test]
    fn rejects_mixed_public_and_private_dns_answers() {
        let resolver = FixedResolver(vec![
            "8.8.8.8:443".parse().unwrap(),
            "127.0.0.1:443".parse().unwrap(),
        ]);
        let error = resolve_public_addrs_with("mixed.example", 443, &resolver, is_private_ip)
            .expect_err("mixed answer must fail closed");
        assert!(error.contains("non-public address"));
    }

    #[test]
    fn deduplicates_public_answers_and_preserves_port() {
        let resolver = FixedResolver(vec![
            "8.8.8.8:8443".parse().unwrap(),
            "8.8.8.8:8443".parse().unwrap(),
        ]);
        assert_eq!(
            resolve_public_addrs_with("public.example", 8443, &resolver, is_private_ip).unwrap(),
            vec!["8.8.8.8:8443".parse().unwrap()]
        );
    }
}
