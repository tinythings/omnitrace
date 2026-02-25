pub(crate) fn hex_port(s: &str) -> Option<u16> {
    u16::from_str_radix(s, 16).ok()
}

pub(crate) fn dec_ipv4(hex_le: &str) -> Option<std::net::Ipv4Addr> {
    let v = u32::from_str_radix(hex_le, 16).ok()?;
    Some(std::net::Ipv4Addr::from(u32::from_le(v)))
}

pub(crate) fn dec_ipv6(hex_be: &str) -> Option<std::net::Ipv6Addr> {
    // /proc/net/tcp6 uses 32 hex chars = 16 bytes in network order
    if hex_be.len() != 32 {
        return None;
    }
    let mut b = [0u8; 16];
    for i in 0..16 {
        b[i] = u8::from_str_radix(&hex_be[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(std::net::Ipv6Addr::from(b))
}

pub(crate) fn decode_addr(raw: &str, v6: bool) -> Option<String> {
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

pub(crate) fn decode_tcp_state(s: &Option<String>) -> Option<String> {
    let code = s.as_deref()?;
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

pub fn reverse_dns(ip: std::net::IpAddr) -> Option<String> {
    use std::ffi::CStr;

    unsafe {
        let mut host = [0i8; 1025];

        let rc = match ip {
            std::net::IpAddr::V4(v4) => {
                let mut sa: libc::sockaddr_in = std::mem::zeroed();
                sa.sin_family = libc::AF_INET as _;
                sa.sin_port = 0;
                // v4 octets are in network order already
                sa.sin_addr = libc::in_addr { s_addr: u32::from_be_bytes(v4.octets()) };

                libc::getnameinfo(
                    (&sa as *const libc::sockaddr_in).cast::<libc::sockaddr>(),
                    std::mem::size_of::<libc::sockaddr_in>() as _,
                    host.as_mut_ptr(),
                    host.len() as _,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }

            std::net::IpAddr::V6(v6) => {
                let mut sa: libc::sockaddr_in6 = std::mem::zeroed();
                sa.sin6_family = libc::AF_INET6 as _;
                sa.sin6_port = 0;
                sa.sin6_addr = libc::in6_addr { s6_addr: v6.octets() };

                libc::getnameinfo(
                    (&sa as *const libc::sockaddr_in6).cast::<libc::sockaddr>(),
                    std::mem::size_of::<libc::sockaddr_in6>() as _,
                    host.as_mut_ptr(),
                    host.len() as _,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        };

        if rc != 0 {
            return None;
        }

        CStr::from_ptr(host.as_ptr()).to_str().ok().map(|s| s.to_string())
    }
}

pub(crate) fn expand_pat(pat: &str) -> String {
    let p = pat.trim();
    if p.is_empty() {
        return String::new();
    }

    // Already explicit DSL
    if p.contains("raw:") || p.contains("dec:") || p.contains("host:") || p.contains("state:") {
        return p.to_string();
    }

    if p == "*" {
        return "*".to_string();
    }

    // Port only
    if p.starts_with(':') && p.len() > 1 {
        return format!("*dec:*{p}*");
    }

    // Pure IPv4
    if p.chars().all(|c| c.is_ascii_digit() || c == '.') && p.contains('.') {
        return format!("*dec:*{p}:*");
    }

    // Pure IPv6 (very loose detection)
    if p.contains(':') && p.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
        return format!("*dec:*{p}:*");
    }

    // Proto
    if p.eq_ignore_ascii_case("tcp") || p.eq_ignore_ascii_case("udp") {
        return format!("{p}*");
    }

    // Default → hostname
    format!("*host:{p}*")
}

pub(crate) fn is_ipish(p: &str) -> bool {
    // allow digits, '.', ':', '*'
    !p.is_empty() && p.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ':' || c == '*')
}

pub(crate) fn is_hostish(p: &str) -> bool {
    // any letter => host
    p.chars().any(|c| c.is_ascii_alphabetic())
        // or has '*' and '.' (typical glob domain)
        || (p.contains('*') && p.contains('.'))
}
