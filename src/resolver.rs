//! A DNS resolver that refuses to hand back addresses inside the local network.
//!
//! `fetch::check_domain` rejects an IP literal, `localhost`, and anything that is not a plain
//! public hostname — but that is string validation, and a name is not an address. `evil.example`
//! is a perfectly well-formed public hostname that can resolve to `127.0.0.1`, `10.0.0.1`, or
//! `169.254.169.254`. For a CLI that is untidy. For an MCP server, whose arguments can come from a
//! repository, an issue, or a README an agent was asked to act on, it is a way into the network the
//! caller is sitting in.
//!
//! The check belongs here rather than before the request for one reason: **ureq connects to
//! exactly the addresses the resolver returns.** Resolving, validating, and then letting the client
//! look the name up a second time leaves a window where the second answer differs from the one that
//! was checked — DNS rebinding. Returning only vetted addresses closes it, because there is no
//! second lookup to poison.
//!
//! Proxies are disabled alongside this. ureq reads `HTTPS_PROXY` and friends from the environment
//! by default, and a proxied request resolves the name at the proxy, not here — which would route
//! around this module entirely.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};

use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::NextTimeout;

/// ureq keeps at most this many addresses from a resolver.
const MAX_ADDRS: usize = 16;

/// Why an address was refused, in words a caller can act on.
pub type Rejections = Arc<Mutex<Vec<String>>>;

/// Whether an address is outside the ranges a public host should never resolve to.
#[must_use]
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        // An IPv4-mapped address such as `::ffff:127.0.0.1` is a v6 value that reaches a v4
        // destination. Checking it as v6 alone would wave loopback straight through.
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_loopback()          // 127/8
        || ip.is_private()      // 10/8, 172.16/12, 192.168/16
        || ip.is_link_local()   // 169.254/16, which is where cloud metadata lives
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()  // 0.0.0.0
        || ip.is_multicast()
        // Ranges std has no predicate for, and which still reach somewhere local:
        || (a == 100 && (64..128).contains(&b))  // 100.64/10  carrier-grade NAT
        || (a == 192 && b == 0 && c == 0)        // 192.0.0/24 IETF protocol assignments
        || (a == 198 && (18..20).contains(&b))   // 198.18/15  benchmarking
        || a >= 240) // 240/4      reserved, includes 255.255.255.255
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_loopback()          // ::1
        || ip.is_unspecified()  // ::
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00   // fc00::/7   unique local
        || (first & 0xffc0) == 0xfe80   // fe80::/10  link local
        || ip.to_ipv4().is_some_and(|v4| !is_public_v4(v4))) // ::a.b.c.d, deprecated but routable
}

/// A resolver that returns only addresses outside the local network.
#[derive(Debug)]
pub struct PublicOnlyResolver {
    rejections: Rejections,
}

impl PublicOnlyResolver {
    /// Creates a resolver that records why it refused an address.
    #[must_use]
    pub fn new(rejections: Rejections) -> Self {
        Self { rejections }
    }
}

impl Resolver for PublicOnlyResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let _ = (config, timeout);
        let scheme = uri.scheme().ok_or(ureq::Error::HostNotFound)?;
        let authority = uri.authority().ok_or(ureq::Error::HostNotFound)?;
        let host_and_port =
            DefaultResolver::host_and_port(scheme, authority).ok_or(ureq::Error::HostNotFound)?;

        let resolved: Vec<SocketAddr> = host_and_port
            .to_socket_addrs()
            .map_err(|_| ureq::Error::HostNotFound)?
            .collect();

        // `ArrayVec` has a fixed capacity and no `clear`; build it empty and push what survives.
        let mut allowed = ResolvedSocketAddrs::from_fn(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
        allowed.truncate(0);
        let mut kept = 0usize;
        let mut refused = Vec::new();
        for address in resolved {
            if is_public(address.ip()) {
                if kept < MAX_ADDRS {
                    allowed.push(address);
                    kept += 1;
                }
            } else {
                refused.push(address.ip().to_string());
            }
        }

        if kept == 0 {
            let detail = if refused.is_empty() {
                format!("{} did not resolve to any address", authority.host())
            } else {
                format!(
                    "{} resolves to {}, which is inside the local network; refusing to connect",
                    authority.host(),
                    refused.join(", ")
                )
            };
            if let Ok(mut rejections) = self.rejections.lock() {
                rejections.push(detail);
            }
            return Err(ureq::Error::HostNotFound);
        }
        Ok(allowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("test address should parse")
    }

    #[test]
    fn local_addresses_are_refused() {
        for text in [
            "127.0.0.1",
            "127.1.2.3",
            "0.0.0.0",
            "10.0.0.5",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254", // the cloud metadata endpoint
            "100.64.0.1",      // carrier-grade NAT
            "192.0.0.1",
            "198.18.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1", // IPv4-mapped loopback
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
        ] {
            assert!(!is_public(ip(text)), "{text} should be refused");
        }
    }

    #[test]
    fn public_addresses_are_allowed() {
        for text in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "172.32.0.1",      // just outside 172.16/12
            "172.15.0.1",      // just below it
            "100.63.0.1",      // just below carrier-grade NAT
            "100.128.0.1",     // just above it
            "192.0.1.1",       // just outside 192.0.0/24
            "198.20.0.1",      // just outside the benchmarking range
            "223.255.255.255", // just below multicast
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8", // IPv4-mapped public
        ] {
            assert!(is_public(ip(text)), "{text} should be allowed");
        }
    }

    #[test]
    fn the_boundaries_of_each_range_are_where_they_should_be() {
        assert!(!is_public(ip("10.255.255.255")));
        assert!(is_public(ip("11.0.0.0")));
        assert!(!is_public(ip("169.254.0.0")));
        assert!(is_public(ip("169.255.0.0")));
        assert!(!is_public(ip("239.0.0.1")), "224/4 is multicast");
        assert!(!is_public(ip("fcff::1")));
        assert!(is_public(ip("fe00::1")), "fe00::/9 is not link local");
    }
}
