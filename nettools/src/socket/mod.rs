use crate::{events, NetTools, SocketBackend};
use omnitrace_core::callbacks::CallbackHub;
use std::{collections::HashSet, io};

pub struct LiveSocketBackend;

impl LiveSocketBackend {
    fn format_socket_addr(ip: String, port: u16, v6: bool) -> String {
        if v6 {
            format!("[{ip}]:{port}")
        } else {
            format!("{ip}:{port}")
        }
    }

    fn hex_port(value: &str) -> Option<u16> {
        u16::from_str_radix(value, 16).ok()
    }

    fn dec_ipv4(value: &str) -> Option<std::net::Ipv4Addr> {
        Some(std::net::Ipv4Addr::from(u32::swap_bytes(u32::from_str_radix(value, 16).ok()?)))
    }

    fn dec_ipv6(value: &str) -> Option<std::net::Ipv6Addr> {
        if value.len() != 32 {
            return None;
        }

        (0..16)
            .map(|index| u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok())
            .collect::<Option<Vec<_>>>()
            .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
            .map(std::net::Ipv6Addr::from)
    }

    pub(crate) fn decode_addr(value: &str, v6: bool) -> Option<String> {
        value
            .split_once(':')
            .and_then(|(ip_hex, port_hex)| Self::hex_port(port_hex).map(|port| (ip_hex, port)))
            .and_then(|(ip_hex, port)| {
                if v6 {
                    Self::dec_ipv6(ip_hex).map(|ip| Self::format_socket_addr(ip.to_string(), port, true))
                } else {
                    Self::dec_ipv4(ip_hex).map(|ip| Self::format_socket_addr(ip.to_string(), port, false))
                }
            })
    }

    pub(crate) fn is_unspecified_remote(remote: &str) -> bool {
        matches!(remote, "0.0.0.0:0" | "[::]:0")
    }

    pub(crate) fn decode_tcp_state(value: Option<&str>) -> Option<String> {
        match value? {
            "01" => Some("ESTABLISHED".to_string()),
            "02" => Some("SYN_SENT".to_string()),
            "03" => Some("SYN_RECV".to_string()),
            "04" => Some("FIN_WAIT1".to_string()),
            "05" => Some("FIN_WAIT2".to_string()),
            "06" => Some("TIME_WAIT".to_string()),
            "07" => Some("CLOSE".to_string()),
            "08" => Some("CLOSE_WAIT".to_string()),
            "09" => Some("LAST_ACK".to_string()),
            "0A" => Some("LISTEN".to_string()),
            "0B" => Some("CLOSING".to_string()),
            _ => Some("UNKNOWN".to_string()),
        }
    }

    fn parse_file(proto: &str, path: &str, is_tcp: bool, out: &mut HashSet<events::SocketEntry>) -> io::Result<()> {
        std::fs::read_to_string(path).map(|content| {
            content.lines().enumerate().skip(1).for_each(|(_, line)| {
                let cs = line.split_whitespace().collect::<Vec<_>>();
                if cs.len() < 3 {
                    return;
                }

                let l = cs[1].to_string();
                let r = cs[2].to_string();
                let s = is_tcp.then(|| cs.get(3).map(|s| (*s).to_string())).flatten();
                let st = Self::decode_tcp_state(s.as_deref());
                let v6 = proto.ends_with('6');
                let ld = Self::decode_addr(&l, v6).unwrap_or(l.clone());
                let rd = Self::decode_addr(&r, v6).unwrap_or(r.clone());
                let k = if st.as_deref() == Some("LISTEN")
                    || (!is_tcp && Self::is_unspecified_remote(&rd))
                {
                    events::SocketKind::Listener
                } else {
                    events::SocketKind::Connection
                };

                out.insert(events::SocketEntry {
                    proto: proto.to_string(),
                    local: ld,
                    remote: rd,
                    state: st,
                    kind: k,
                });
            });
        })
    }
}

impl SocketBackend for LiveSocketBackend {
    fn list(&self) -> io::Result<HashSet<events::SocketEntry>> {
        let mut out = HashSet::new();
        let _ = Self::parse_file("tcp", "/proc/net/tcp", true, &mut out);
        let _ = Self::parse_file("tcp6", "/proc/net/tcp6", true, &mut out);
        let _ = Self::parse_file("udp", "/proc/net/udp", false, &mut out);
        let _ = Self::parse_file("udp6", "/proc/net/udp6", false, &mut out);
        Ok(out)
    }
}

impl NetTools {
    pub(crate) fn poll_sockets(&self) -> io::Result<HashSet<events::SocketEntry>> {
        self.socket_backend.list()
    }

    pub(crate) async fn handle_socket_poll(&mut self, hub: &CallbackHub<events::NetToolsEvent>) {
        let cur = match self.poll_sockets() {
            Ok(cur) => cur,
            Err(err) => {
                log::error!("nettools: failed to read sockets: {err}");
                return;
            }
        };

        for s in cur.difference(&self.last_sockets) {
            Self::fire(
                hub,
                events::NetToolsEvent::SocketAdded {
                    socket: s.clone(),
                },
            )
            .await;
        }

        for s in self.last_sockets.difference(&cur) {
            Self::fire(
                hub,
                events::NetToolsEvent::SocketRemoved {
                    socket: s.clone(),
                },
            )
            .await;
        }

        self.last_sockets = cur;
    }
}
