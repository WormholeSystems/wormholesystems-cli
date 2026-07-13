//! Best-effort network introspection for the DNS guidance: this machine's
//! public IP and what a hostname currently resolves to.

use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(4);

/// Services that echo the caller's IP as plain text. Queried over HTTP
/// (not HTTPS) to stay stdlib-only; a public IP is not a secret.
const IP_ECHO_HOSTS: [&str; 3] = ["checkip.amazonaws.com", "api.ipify.org", "ifconfig.me"];

pub fn public_ip() -> Option<IpAddr> {
    IP_ECHO_HOSTS
        .iter()
        .find_map(|host| http_get_body(host)?.trim().parse().ok())
}

/// All addresses `host` currently resolves to via the system resolver.
pub fn resolve(host: &str) -> Vec<IpAddr> {
    let mut ips: Vec<IpAddr> = (host, 80)
        .to_socket_addrs()
        .map(|addrs| addrs.map(|a| a.ip()).collect())
        .unwrap_or_default();
    ips.sort();
    ips.dedup();
    ips
}

/// Minimal GET; HTTP/1.0 so the body is never chunked and ends at EOF.
fn http_get_body(host: &str) -> Option<String> {
    let addr = (host, 80).to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(TIMEOUT)).ok()?;
    write!(
        stream,
        "GET / HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).ok()?;
    body_of(&String::from_utf8_lossy(&response)).map(str::to_string)
}

/// The body of a 200 response; None for any other status.
fn body_of(response: &str) -> Option<&str> {
    let (head, body) = response.split_once("\r\n\r\n")?;
    let status = head.split_whitespace().nth(1)?;
    (status == "200").then_some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_of_accepts_only_200() {
        let ok = "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\n203.0.113.7\n";
        assert_eq!(body_of(ok), Some("203.0.113.7\n"));
        let redirect = "HTTP/1.0 301 Moved\r\nLocation: x\r\n\r\nmoved";
        assert_eq!(body_of(redirect), None);
        assert_eq!(body_of("garbage"), None);
    }

    #[test]
    fn resolve_finds_localhost() {
        let ips = resolve("localhost");
        assert!(ips.iter().any(|ip| ip.is_loopback()));
    }

    #[test]
    fn resolve_returns_empty_for_unresolvable_host() {
        assert!(resolve("this-host-does-not-exist.invalid").is_empty());
    }
}
