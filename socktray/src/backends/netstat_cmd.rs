use crate::SockBackend;
use crate::events::SockKey;
use std::collections::HashSet;
use std::io;
use tokio::process::Command;

pub struct NetstatBackend;

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
    let (local, remote, state) = if proto.starts_with("tcp") && parts.len() >= 6 {
        (
            parts[parts.len() - 3].to_string(),
            parts[parts.len() - 2].to_string(),
            Some(parts[parts.len() - 1].to_string()),
        )
    } else {
        (parts[parts.len() - 2].to_string(), parts[parts.len() - 1].to_string(), None)
    };

    Some(SockKey {
        proto,
        local,
        remote,
        state,
        local_dec: None,
        remote_dec: None,
        state_dec: None,
    })
}

#[async_trait::async_trait]
impl SockBackend for NetstatBackend {
    async fn list(&self) -> io::Result<HashSet<SockKey>> {
        let out = Command::new("netstat").args(["-an"]).output().await?;
        if !out.status.success() {
            return Ok(HashSet::new());
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut result = HashSet::new();
        for line in stdout.lines() {
            if let Some(sk) = parse_netstat_line(line) {
                result.insert(sk);
            }
        }
        Ok(result)
    }
}
