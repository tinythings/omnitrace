use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use pnet::{
    datalink::{self, Channel, Config},
    packet::{
        Packet,
        ethernet::{EtherTypes, EthernetPacket},
        ip::IpNextHeaderProtocols,
        ipv4::Ipv4Packet,
        ipv6::Ipv6Packet,
        tcp::TcpPacket,
    },
};

pub type SniKey = (IpAddr, u16, IpAddr, u16);
pub type SniVal = (String, Instant);
pub type SniCacheMap = HashMap<SniKey, SniVal>;
pub type SniCache = Arc<Mutex<SniCacheMap>>;
type FlowBufMap = HashMap<SniKey, (Vec<u8>, Instant)>;

static SNI_CACHE: OnceLock<SniCache> = OnceLock::new();
static SNI_STARTED: OnceLock<()> = OnceLock::new();

pub fn sni_cache() -> SniCache {
    let cache = SNI_CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new()))).clone();

    SNI_STARTED.get_or_init(|| {
        let c = cache.clone();
        std::thread::spawn(move || run_sni_sniffer(c, None));
    });

    cache
}

pub fn lookup_sni(key: SniKey, ttl: Duration) -> Option<String> {
    let cache = sni_cache();
    let now = Instant::now();

    let mut map = cache.lock().unwrap();
    map.retain(|_, (_, ts)| now.duration_since(*ts) < ttl);
    map.get(&key).map(|(s, _)| s.clone())
}

pub fn run_sni_sniffer(
    cache: SniCache,
    iface_name: Option<String>,
) {
    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::time::{Duration, Instant};

    let ifaces: Vec<_> = pnet::datalink::interfaces()
        .into_iter()
        .filter(|i| {
            if !i.is_up() {
                return false;
            }

            if let Some(name) = iface_name.as_deref() {
                i.name == name
            } else {
                !i.is_loopback()
            }
        })
        .collect();

    if ifaces.is_empty() {
        if let Some(name) = iface_name {
            log::warn!("netnotify: requested SNI interface '{name}' not found or not up");
        } else {
            log::warn!("netnotify: no UP non-loopback interface available for SNI sniffing");
        }
        return;
    }

    for iface in ifaces {
        let cache = cache.clone();

        std::thread::spawn(move || {
            let cfg = pnet::datalink::Config {
                promiscuous: false,
                read_timeout: Some(Duration::from_millis(50)),
                read_buffer_size: 1 << 20,
                ..Default::default()
            };

            let chan = match pnet::datalink::channel(&iface, cfg) {
                Ok(pnet::datalink::Channel::Ethernet(_, rx)) => rx,
                _ => return,
            };

            let mut rx = chan;
            let mut flows: FlowBufMap = HashMap::new();
            let mut last_gc = Instant::now();

            loop {
                // periodic GC
                if last_gc.elapsed() > Duration::from_secs(2) {
                    let now = Instant::now();
                    flows.retain(|_, (_, ts)| now.duration_since(*ts) < Duration::from_secs(10));
                    last_gc = now;
                }

                let pkt = match rx.next() {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                let eth = match pnet::packet::ethernet::EthernetPacket::new(pkt) {
                    Some(e) => e,
                    None => continue,
                };

                let (src, dst, sport, dport, payload) = match eth.get_ethertype() {
                    pnet::packet::ethernet::EtherTypes::Ipv4 => {
                        let ip = match pnet::packet::ipv4::Ipv4Packet::new(eth.payload()) {
                            Some(p) => p,
                            None => continue,
                        };
                        if ip.get_next_level_protocol() != pnet::packet::ip::IpNextHeaderProtocols::Tcp {
                            continue;
                        }
                        let tcp = match pnet::packet::tcp::TcpPacket::new(ip.payload()) {
                            Some(t) => t,
                            None => continue,
                        };

                        let payload = tcp.payload();
                        if payload.is_empty() {
                            continue;
                        }

                        (IpAddr::V4(ip.get_source()), IpAddr::V4(ip.get_destination()), tcp.get_source(), tcp.get_destination(), payload.to_vec())
                    }

                    pnet::packet::ethernet::EtherTypes::Ipv6 => {
                        let ip = match pnet::packet::ipv6::Ipv6Packet::new(eth.payload()) {
                            Some(p) => p,
                            None => continue,
                        };
                        if ip.get_next_header() != pnet::packet::ip::IpNextHeaderProtocols::Tcp {
                            continue;
                        }
                        let tcp = match pnet::packet::tcp::TcpPacket::new(ip.payload()) {
                            Some(t) => t,
                            None => continue,
                        };

                        let payload = tcp.payload();
                        if payload.is_empty() {
                            continue;
                        }

                        (IpAddr::V6(ip.get_source()), IpAddr::V6(ip.get_destination()), tcp.get_source(), tcp.get_destination(), payload.to_vec())
                    }

                    _ => continue,
                };

                // We only care about ClientHello direction: client -> server:443
                if dport != 443 {
                    continue;
                }

                let key = (src, sport, dst, dport);
                let entry = flows.entry(key).or_insert_with(|| (Vec::with_capacity(4096), Instant::now()));
                entry.1 = Instant::now();

                // append + cap
                if entry.0.len() + payload.len() <= 64 * 1024 {
                    entry.0.extend_from_slice(&payload);
                } else {
                    flows.remove(&key);
                    continue;
                }

                // try decode from reassembled buffer
                if let Some(sni) = extract_sni(&entry.0) {
                    eprintln!("SNI: {sni}  flow={src}:{sport} -> {dst}:{dport}");

                    let now = Instant::now();
                    let mut map = cache.lock().unwrap();

                    // store both directions so lookup works no matter how you key it
                    map.insert((src, sport, dst, dport), (sni.clone(), now));
                    map.insert((dst, dport, src, sport), (sni.clone(), now));

                    flows.remove(&key);
                }
            }
        });
    }
}

fn extract_sni(payload: &[u8]) -> Option<String> {
    if payload.len() < 5 {
        return None;
    }

    // TLS record type 22 = Handshake
    if payload[0] != 22 {
        return None;
    }

    // Skip record header (5 bytes)
    let mut i = 5;

    // Handshake type 1 = ClientHello
    if payload.get(i)? != &1 {
        return None;
    }

    i += 4; // skip handshake header

    i += 2; // version
    i += 32; // random

    let session_len = *payload.get(i)? as usize;
    i += 1 + session_len;

    let cipher_len = u16::from_be_bytes([payload[i], payload[i + 1]]) as usize;
    i += 2 + cipher_len;

    let comp_len = *payload.get(i)? as usize;
    i += 1 + comp_len;

    let ext_len = u16::from_be_bytes([payload[i], payload[i + 1]]) as usize;
    i += 2;

    let end = i + ext_len;

    while i + 4 <= end {
        let ext_type = u16::from_be_bytes([payload[i], payload[i + 1]]);
        let ext_size = u16::from_be_bytes([payload[i + 2], payload[i + 3]]) as usize;
        i += 4;

        if ext_type == 0 {
            // server_name extension
            let list_len = u16::from_be_bytes([payload[i], payload[i + 1]]) as usize;
            let mut j = i + 2;

            while j < i + 2 + list_len {
                let name_type = payload[j];
                let name_len = u16::from_be_bytes([payload[j + 1], payload[j + 2]]) as usize;
                j += 3;

                if name_type == 0 {
                    return std::str::from_utf8(&payload[j..j + name_len]).ok().map(|s| s.to_string());
                }
                j += name_len;
            }
        }

        i += ext_size;
    }

    None
}

// ---- Sniff SNI for one TCP flow (Linux, layer2 capture) ----
//
// We capture outgoing packets on the interface that has `local_ip`,
// filter by 4-tuple, concatenate TCP payload until we can parse ClientHello.
//
// This is best-effort: no full TCP reassembly, but works in practice for “first packets”.
pub fn sniff_sni_for_flow(local_ip: IpAddr, local_port: u16, remote_ip: IpAddr, remote_port: u16, timeout: Duration) -> Option<String> {
    // TLS record header: 16 03 01|03|04 ...
    #[inline]
    fn find_tls_start(p: &[u8]) -> Option<usize> {
        p.windows(3).position(|w| w[0] == 22 && w[1] == 3 && (1..=4).contains(&w[2]))
    }

    let iface = datalink::interfaces().into_iter().find(|ifc| ifc.ips.iter().any(|ipn| ipn.ip() == local_ip))?;

    let cfg = Config {
        promiscuous: false,
        read_timeout: Some(Duration::from_millis(50)),
        read_buffer_size: 1 << 20,
        write_buffer_size: 1 << 16,
        ..Default::default()
    };

    let channel = datalink::channel(&iface, cfg).ok()?;

    let mut rx = match channel {
        Channel::Ethernet(_tx, rx) => rx,
        _ => return None,
    };

    let deadline = std::time::Instant::now() + timeout;
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut started_tls = false;

    while std::time::Instant::now() < deadline {
        let pkt = match rx.next() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let eth = match EthernetPacket::new(pkt) {
            Some(e) => e,
            None => continue,
        };

        match eth.get_ethertype() {
            EtherTypes::Ipv4 => {
                let Some(ip) = Ipv4Packet::new(eth.payload()) else { continue };
                if ip.get_next_level_protocol() != IpNextHeaderProtocols::Tcp {
                    continue;
                }

                let src = IpAddr::V4(ip.get_source());
                let dst = IpAddr::V4(ip.get_destination());
                if src != local_ip || dst != remote_ip {
                    continue;
                }

                let Some(tcp) = TcpPacket::new(ip.payload()) else { continue };
                if tcp.get_source() != local_port || tcp.get_destination() != remote_port {
                    continue;
                }

                let payload = tcp.payload();
                if payload.is_empty() {
                    continue;
                }

                if !started_tls {
                    let Some(pos) = find_tls_start(payload) else { continue };
                    started_tls = true;
                    buf.extend_from_slice(&payload[pos..]);
                } else {
                    buf.extend_from_slice(payload);
                }
            }

            EtherTypes::Ipv6 => {
                let Some(ip) = Ipv6Packet::new(eth.payload()) else { continue };
                if ip.get_next_header() != IpNextHeaderProtocols::Tcp {
                    continue;
                }

                let src = IpAddr::V6(ip.get_source());
                let dst = IpAddr::V6(ip.get_destination());
                if src != local_ip || dst != remote_ip {
                    continue;
                }

                let Some(tcp) = TcpPacket::new(ip.payload()) else { continue };
                if tcp.get_source() != local_port || tcp.get_destination() != remote_port {
                    continue;
                }

                let payload = tcp.payload();
                if payload.is_empty() {
                    continue;
                }

                if !started_tls {
                    let Some(pos) = find_tls_start(payload) else { continue };
                    started_tls = true;
                    buf.extend_from_slice(&payload[pos..]);
                } else {
                    buf.extend_from_slice(payload);
                }
            }

            _ => continue,
        }

        if let Some(sni) = extract_sni(&buf) {
            return Some(sni);
        }

        if buf.len() > 96 * 1024 {
            return None;
        }
    }

    None
}
