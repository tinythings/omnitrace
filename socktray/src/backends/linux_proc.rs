use crate::SockBackend;
use crate::events::SockKey;
use std::collections::HashSet;
use std::io;

pub struct LinuxProcBackend;

fn hex_port(s: &str) -> Option<u16> {
    u16::from_str_radix(s, 16).ok()
}

fn dec_ipv4(hex_le: &str) -> Option<std::net::Ipv4Addr> {
    let v = u32::from_str_radix(hex_le, 16).ok()?;
    Some(std::net::Ipv4Addr::from(u32::swap_bytes(v)))
}

fn dec_ipv6(hex_be: &str) -> Option<std::net::Ipv6Addr> {
    if hex_be.len() != 32 {
        return None;
    }
    let mut b = [0u8; 16];
    for i in 0..16 {
        b[i] = u8::from_str_radix(&hex_be[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(std::net::Ipv6Addr::from(b))
}

fn decode_addr(raw: &str, v6: bool) -> Option<String> {
    let (ip_hex, port_hex) = raw.split_once(':')?;
    let port = hex_port(port_hex)?;
    if v6 {
        let ip = dec_ipv6(ip_hex)?;
        Some(format!("{ip}:{port}"))
    } else {
        let ip = dec_ipv4(ip_hex)?;
        Some(format!("{ip}:{port}"))
    }
}

fn decode_tcp_state(s: Option<&str>) -> Option<String> {
    let code = s?;
    let name = match code {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        _ => "UNKNOWN",
    };
    Some(name.to_string())
}

fn parse_file(proto: &str, path: &str, is_tcp: bool, out: &mut HashSet<SockKey>) -> io::Result<()> {
    let txt = std::fs::read_to_string(path)?;
    for (i, line) in txt.lines().enumerate() {
        if i == 0 {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 3 {
            continue;
        }

        let local = cols[1].to_string();
        let remote = cols[2].to_string();
        let state = if is_tcp { cols.get(3).map(|s| s.to_string()) } else { None };

        let is_v6 = proto.ends_with('6');
        let local_dec = decode_addr(&local, is_v6);
        let remote_dec = decode_addr(&remote, is_v6);
        let state_dec = decode_tcp_state(state.as_deref());

        out.insert(SockKey { proto: proto.to_string(), local, remote, state, local_dec, remote_dec, state_dec, remote_host: None });
    }
    Ok(())
}

#[async_trait::async_trait]
impl SockBackend for LinuxProcBackend {
    async fn list(&self) -> io::Result<HashSet<SockKey>> {
        let mut out = HashSet::new();

        let _ = parse_file("tcp", "/proc/net/tcp", true, &mut out);
        let _ = parse_file("tcp6", "/proc/net/tcp6", true, &mut out);
        let _ = parse_file("udp", "/proc/net/udp", false, &mut out);
        let _ = parse_file("udp6", "/proc/net/udp6", false, &mut out);

        Ok(out)
    }
}
