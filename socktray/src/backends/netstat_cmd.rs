use crate::SockBackend;
use crate::events::SockKey;
use std::collections::HashSet;
use std::io;
use std::net::IpAddr;
use tokio::process::Command;

pub struct NetstatBackend;

fn split_host_port(ep: &str) -> Option<(String, u16)> {
    let ep = ep.trim();
    if ep.is_empty() || ep == "*" || ep == "*.*" || ep == "*:*" {
        return None;
    }

    // [ipv6]:443 or [ipv6].443
    if let Some(rest) = ep.strip_prefix('[')
        && let Some((host, tail)) = rest.split_once(']')
    {
        let port = tail.strip_prefix(':').or_else(|| tail.strip_prefix('.'))?;
        let port: u16 = port.parse().ok()?;
        return Some((host.to_string(), port));
    }

    // ip:port
    if let Some((host, port)) = ep.rsplit_once(':')
        && let Ok(port) = port.parse::<u16>()
    {
        return Some((host.to_string(), port));
    }

    // NetBSD/FreeBSD netstat often prints ipv4 as a.b.c.d.port
    if let Some((host, port)) = ep.rsplit_once('.')
        && let Ok(port) = port.parse::<u16>()
    {
        return Some((host.to_string(), port));
    }

    None
}

fn endpoint_dec(ep: &str) -> Option<String> {
    let (host, port) = split_host_port(ep)?;
    let ip: IpAddr = host.parse().ok()?;
    Some(format!("{ip}:{port}"))
}

fn parse_netstat_line(line: &str) -> Option<SockKey> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }

    let proto = parts[0].to_lowercase();
    if !proto.starts_with("tcp") && !proto.starts_with("udp") {
        return None;
    }

    // This is a portable-ish approximation across BSD netstat outputs.
    let (local, remote, state, state_dec) = if proto.starts_with("tcp") && parts.len() >= 6 {
        let state = parts[parts.len() - 1].to_string();
        (
            parts[parts.len() - 3].to_string(),
            parts[parts.len() - 2].to_string(),
            Some(state.clone()),
            Some(state),
        )
    } else {
        (parts[parts.len() - 2].to_string(), parts[parts.len() - 1].to_string(), None, None)
    };

    let local_dec = endpoint_dec(&local);
    let remote_dec = endpoint_dec(&remote);

    Some(SockKey { proto, local, remote, state, local_dec, remote_dec, state_dec, remote_host: None })
}

#[async_trait::async_trait]
impl SockBackend for NetstatBackend {
    async fn list(&self) -> io::Result<HashSet<SockKey>> {
        #[cfg(any(target_os = "netbsd", target_os = "freebsd"))]
        {
            let mut result = HashSet::new();
            let outs = [
                Command::new("netstat").args(["-an", "-f", "inet"]).output().await,
                Command::new("netstat").args(["-an", "-f", "inet6"]).output().await,
            ];

            for out in outs {
                let Ok(out) = out else {
                    continue;
                };
                if !out.status.success() {
                    continue;
                }
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    if let Some(sk) = parse_netstat_line(line) {
                        result.insert(sk);
                    }
                }
            }
            return Ok(result);
        }

        #[cfg(not(any(target_os = "netbsd", target_os = "freebsd")))]
        {
            let mut result = HashSet::new();
            let out = Command::new("netstat").args(["-an"]).output().await?;
            if !out.status.success() {
                return Ok(result);
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if let Some(sk) = parse_netstat_line(line) {
                    result.insert(sk);
                }
            }
            return Ok(result);
        }
    }
}
