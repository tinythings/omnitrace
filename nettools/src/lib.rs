pub mod events;

#[cfg(test)]
mod nettools_ut;

use crate::events::NetToolsEvent;
use async_trait::async_trait;
use omnitrace_core::{
    callbacks::CallbackHub,
    sensor::{Sensor, SensorCtx},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    io,
    net::ToSocketAddrs,
    pin::Pin,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::TcpStream, time::timeout};

pub trait HostnameBackend: Send + Sync {
    fn current(&self) -> io::Result<String>;
}

pub trait RouteBackend: Send + Sync {
    fn list(&self) -> io::Result<Vec<events::RouteEntry>>;
}

#[async_trait]
pub trait NetHealthBackend: Send + Sync {
    async fn probe(&self, target: &events::NetHealthTarget, probe_timeout: Duration) -> io::Result<Duration>;
}

pub trait SocketBackend: Send + Sync {
    fn list(&self) -> io::Result<HashSet<events::SocketEntry>>;
}

pub trait NeighbourBackend: Send + Sync {
    fn list(&self) -> io::Result<HashMap<String, events::NeighbourEntry>>;
}

pub trait ThroughputBackend: Send + Sync {
    fn list(&self) -> io::Result<HashMap<String, events::InterfaceCounters>>;
}

pub trait WifiBackend: Send + Sync {
    fn list(&self) -> io::Result<HashMap<String, events::WifiDetails>>;
}

pub struct LiveHostnameBackend;
pub struct LiveRouteBackend;
pub struct LiveNetHealthBackend;
pub struct LiveSocketBackend;
pub struct LiveNeighbourBackend;
pub struct LiveThroughputBackend;
pub struct LiveWifiBackend;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct RouteKey {
    family: events::RouteFamily,
    destination: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct RouteLookupKey {
    target: String,
}

#[derive(Clone, Debug)]
struct NetHealthSample {
    total_probes: usize,
    successful_probes: usize,
    latency_sum_ms: u64,
}

#[derive(Clone, Debug)]
struct ThroughputState {
    at: Instant,
    counters: HashMap<String, events::InterfaceCounters>,
}

impl LiveHostnameBackend {
    fn read_hostname() -> io::Result<String> {
        let mut buf = [0u8; 256];
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast::<libc::c_char>(), buf.len()) };

        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(String::from_utf8_lossy(&buf[..buf.iter().position(|b| *b == 0).unwrap_or(buf.len())]).trim().to_string())
    }
}

impl HostnameBackend for LiveHostnameBackend {
    fn current(&self) -> io::Result<String> {
        Self::read_hostname()
    }
}

impl LiveRouteBackend {
    fn current_family(line: &str, current_family: &events::RouteFamily) -> events::RouteFamily {
        if line.starts_with("Internet6:") || line.starts_with("Kernel IPv6 routing table") {
            return events::RouteFamily::Inet6;
        }

        if line.starts_with("Internet:") || line.starts_with("Kernel IP routing table") {
            return events::RouteFamily::Inet;
        }

        current_family.clone()
    }

    fn parse_line(line: &str, current_family: &events::RouteFamily) -> Option<events::RouteEntry> {
        if line.is_empty()
            || line.starts_with("Destination")
            || line.starts_with("Routing")
            || line.starts_with("Kernel")
            || line.starts_with("Internet")
        {
            return None;
        }

        let fields = line.split_whitespace().collect::<Vec<_>>();

        if fields.len() < 3 {
            return None;
        }

        Some(events::RouteEntry {
            family: if matches!(current_family, events::RouteFamily::Unknown) {
                if fields.first().is_some_and(|value| value.contains(':')) || fields.get(1).is_some_and(|value| value.contains(':')) {
                    events::RouteFamily::Inet6
                } else {
                    events::RouteFamily::Inet
                }
            } else {
                current_family.clone()
            },
            destination: fields.first().unwrap_or(&"").to_string(),
            gateway: fields.get(1).unwrap_or(&"").to_string(),
            iface: fields.last().unwrap_or(&"").to_string(),
        })
    }

    fn parse_routes(output: &str) -> Vec<events::RouteEntry> {
        output
            .lines()
            .fold((Vec::new(), events::RouteFamily::Unknown), |(mut routes, current_family), line| {
                let updated_family = Self::current_family(line.trim(), &current_family);

                if let Some(route) = Self::parse_line(line.trim(), &updated_family) {
                    routes.push(route);
                }

                (routes, updated_family)
            })
            .0
    }
}

impl RouteBackend for LiveRouteBackend {
    fn list(&self) -> io::Result<Vec<events::RouteEntry>> {
        Command::new("netstat")
            .arg("-rn")
            .output()
            .map_err(io::Error::other)
            .and_then(|output| {
                if output.status.success() {
                    Ok(Self::parse_routes(&String::from_utf8_lossy(&output.stdout)))
                } else {
                    Err(io::Error::other(String::from_utf8_lossy(&output.stderr).to_string()))
                }
            })
    }
}

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
        let parsed = u32::from_str_radix(value, 16).ok()?;
        Some(std::net::Ipv4Addr::from(u32::swap_bytes(parsed)))
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

    fn decode_addr(value: &str, v6: bool) -> Option<String> {
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

    fn is_unspecified_remote(remote: &str) -> bool {
        matches!(remote, "0.0.0.0:0" | "[::]:0")
    }

    fn decode_tcp_state(value: Option<&str>) -> Option<String> {
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
                let cols = line.split_whitespace().collect::<Vec<_>>();
                if cols.len() < 3 {
                    return;
                }

                let local = cols[1].to_string();
                let remote = cols[2].to_string();
                let state = is_tcp.then(|| cols.get(3).map(|state| (*state).to_string())).flatten();
                let state_dec = Self::decode_tcp_state(state.as_deref());
                let is_v6 = proto.ends_with('6');
                let local_dec = Self::decode_addr(&local, is_v6).unwrap_or(local.clone());
                let remote_dec = Self::decode_addr(&remote, is_v6).unwrap_or(remote.clone());
                let kind = if state_dec.as_deref() == Some("LISTEN")
                    || (!is_tcp && Self::is_unspecified_remote(&remote_dec))
                {
                    events::SocketKind::Listener
                } else {
                    events::SocketKind::Connection
                };

                out.insert(events::SocketEntry {
                    proto: proto.to_string(),
                    local: local_dec,
                    remote: remote_dec,
                    state: state_dec,
                    kind,
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

impl LiveNeighbourBackend {
    fn parse_proc_net_arp(content: &str) -> HashMap<String, events::NeighbourEntry> {
        content
            .lines()
            .enumerate()
            .skip(1)
            .filter_map(|(_, line)| {
                let cols = line.split_whitespace().collect::<Vec<_>>();
                (cols.len() >= 6).then(|| {
                    (
                        cols[0].to_string(),
                        events::NeighbourEntry {
                            address: cols[0].to_string(),
                            mac: cols[3].to_string(),
                            iface: cols[5].to_string(),
                            state: Some(cols[2].to_string()),
                        },
                    )
                })
            })
            .collect()
    }

    fn parse_arp_line(line: &str) -> Option<events::NeighbourEntry> {
        let address = line
            .split_whitespace()
            .find(|field| field.parse::<std::net::IpAddr>().is_ok())
            .map(str::to_string)?;
        let mac = line
            .split_whitespace()
            .find(|field| field.chars().filter(|ch| *ch == ':').count() == 5)
            .map(str::to_string)?;
        let iface = line
            .split_whitespace()
            .rev()
            .find(|field| field.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.'))
            .map(str::to_string)
            .unwrap_or_default();

        Some(events::NeighbourEntry {
            address: address.clone(),
            mac,
            iface,
            state: None,
        })
    }

    fn parse_ip_neigh_line(line: &str) -> Option<events::NeighbourEntry> {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let address = fields
            .first()
            .filter(|field| field.parse::<std::net::IpAddr>().is_ok())
            .map(|field| (*field).to_string())?;
        let iface = fields
            .windows(2)
            .find(|window| window[0] == "dev")
            .map(|window| window[1].to_string())
            .unwrap_or_default();
        let mac = fields
            .windows(2)
            .find(|window| matches!(window[0], "lladdr" | "at"))
            .map(|window| window[1].to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let state = fields
            .iter()
            .rev()
            .find(|field| field.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_'))
            .map(|field| (*field).to_string());

        Some(events::NeighbourEntry {
            address: address.clone(),
            mac,
            iface,
            state,
        })
    }

    fn parse_neighbour_table(content: &str) -> HashMap<String, events::NeighbourEntry> {
        content
            .lines()
            .filter_map(Self::parse_ip_neigh_line)
            .map(|neighbour| (neighbour.address.clone(), neighbour))
            .collect()
    }
}

impl NeighbourBackend for LiveNeighbourBackend {
    fn list(&self) -> io::Result<HashMap<String, events::NeighbourEntry>> {
        std::fs::read_to_string("/proc/net/arp")
            .map(|content| Self::parse_proc_net_arp(&content))
            .or_else(|_| {
                std::fs::read_to_string("/proc/net/ndisc_cache")
                    .map(|content| Self::parse_neighbour_table(&content))
            })
            .or_else(|_| {
                Command::new("ip")
                    .arg("neigh")
                    .output()
                    .map_err(io::Error::other)
                    .and_then(|output| {
                        if output.status.success() {
                            Ok(Self::parse_neighbour_table(&String::from_utf8_lossy(&output.stdout)))
                        } else {
                            Err(io::Error::other(String::from_utf8_lossy(&output.stderr).to_string()))
                        }
                    })
            })
            .or_else(|_| {
                Command::new("arp")
                    .arg("-an")
                    .output()
                    .map_err(io::Error::other)
                    .and_then(|output| {
                        if output.status.success() {
                            Ok(String::from_utf8_lossy(&output.stdout)
                                .lines()
                                .filter_map(Self::parse_arp_line)
                                .map(|neighbour| (neighbour.address.clone(), neighbour))
                                .collect())
                        } else {
                            Err(io::Error::other(String::from_utf8_lossy(&output.stderr).to_string()))
                        }
                    })
            })
    }
}

impl LiveThroughputBackend {
    fn parse_proc_net_dev_line(line: &str) -> Option<events::InterfaceCounters> {
        line.split_once(':').and_then(|(iface, stats)| {
            stats
                .split_whitespace()
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()
                .filter(|values| values.len() >= 16)
                .map(|values| events::InterfaceCounters {
                    iface: iface.trim().to_string(),
                    rx_bytes: values[0],
                    rx_packets: values[1],
                    rx_errors: values[2],
                    rx_drops: values[3],
                    tx_bytes: values[8],
                    tx_packets: values[9],
                    tx_errors: values[10],
                    tx_drops: values[11],
                })
        })
    }

    fn parse_proc_net_dev(content: &str) -> HashMap<String, events::InterfaceCounters> {
        content
            .lines()
            .skip(2)
            .filter_map(Self::parse_proc_net_dev_line)
            .map(|counters| (counters.iface.clone(), counters))
            .collect()
    }

    fn parse_netstat_ib_line(line: &str) -> Option<events::InterfaceCounters> {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        (fields.len() >= 10)
            .then(|| events::InterfaceCounters {
                iface: fields[0].to_string(),
                rx_packets: fields.iter().rev().nth(3).and_then(|v| v.parse().ok()).unwrap_or(0),
                rx_errors: 0,
                rx_drops: 0,
                tx_packets: fields.iter().rev().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0),
                tx_errors: 0,
                tx_drops: 0,
                rx_bytes: fields.iter().rev().nth(2).and_then(|v| v.parse().ok()).unwrap_or(0),
                tx_bytes: fields.last().and_then(|v| v.parse().ok()).unwrap_or(0),
            })
            .filter(|counters| !matches!(counters.iface.as_str(), "Name" | "Iface"))
    }

    fn parse_netstat_ib(content: &str) -> HashMap<String, events::InterfaceCounters> {
        content
            .lines()
            .filter_map(Self::parse_netstat_ib_line)
            .map(|counters| (counters.iface.clone(), counters))
            .collect()
    }
}

impl ThroughputBackend for LiveThroughputBackend {
    fn list(&self) -> io::Result<HashMap<String, events::InterfaceCounters>> {
        std::fs::read_to_string("/proc/net/dev")
            .map(|content| Self::parse_proc_net_dev(&content))
            .or_else(|_| {
                Command::new("netstat")
                    .args(["-ibn"])
                    .output()
                    .map_err(io::Error::other)
                    .and_then(|output| {
                        if output.status.success() {
                            Ok(Self::parse_netstat_ib(&String::from_utf8_lossy(&output.stdout)))
                        } else {
                            Err(io::Error::other(String::from_utf8_lossy(&output.stderr).to_string()))
                        }
                    })
            })
    }
}

impl LiveWifiBackend {
    fn parse_wireless_float(value: &str) -> f32 {
        value.trim_end_matches('.').parse::<f32>().unwrap_or(0.0)
    }

    fn parse_wireless_line(line: &str) -> Option<events::WifiDetails> {
        line.split_once(':').and_then(|(iface, stats)| {
            let fields = stats.split_whitespace().collect::<Vec<_>>();
            (fields.len() >= 4).then(|| events::WifiDetails {
                iface: iface.trim().to_string(),
                connected: fields[0] != "0000",
                link_quality: Self::parse_wireless_float(fields[1]),
                signal_level_dbm: Self::parse_wireless_float(fields[2]),
                noise_level_dbm: Self::parse_wireless_float(fields[3]),
                ssid: None,
                bssid: None,
            })
        })
    }

    fn enrich_from_iw_command(iface: &str, wifi: &mut events::WifiDetails) {
        let iw_output = ["/usr/sbin/iw", "/usr/bin/iw"]
            .iter()
            .find_map(|path| {
                std::path::Path::new(path)
                    .exists()
                    .then(|| Command::new(path).args(["dev", iface, "link"]).output().ok())
                    .flatten()
            });

        if let Some(output) = iw_output {
            let text = String::from_utf8_lossy(&output.stdout);

            if text.lines().any(|line| line.trim() == "Not connected.") {
                wifi.connected = false;
                wifi.ssid = None;
                wifi.bssid = None;
                return;
            }

            text.lines().for_each(|line| {
                let trimmed = line.trim();

                if let Some(bssid) = trimmed.strip_prefix("Connected to ") {
                    wifi.bssid = bssid
                        .split_whitespace()
                        .next()
                        .map(str::to_string);
                } else if let Some(ssid) = trimmed.strip_prefix("SSID: ") {
                    wifi.ssid = Some(ssid.to_string());
                } else if let Some(signal) = trimmed.strip_prefix("signal: ") {
                    wifi.signal_level_dbm = signal
                        .split_whitespace()
                        .next()
                        .and_then(|value| value.parse::<f32>().ok())
                        .unwrap_or(wifi.signal_level_dbm);
                }
            });
        }
    }

    fn parse_proc_net_wireless(content: &str) -> HashMap<String, events::WifiDetails> {
        content
            .lines()
            .skip(2)
            .filter_map(Self::parse_wireless_line)
            .map(|mut wifi| {
                let iface = wifi.iface.clone();
                Self::enrich_from_iw_command(&iface, &mut wifi);
                (iface, wifi)
            })
            .collect()
    }
}

impl WifiBackend for LiveWifiBackend {
    fn list(&self) -> io::Result<HashMap<String, events::WifiDetails>> {
        std::fs::read_to_string("/proc/net/wireless").map(|content| Self::parse_proc_net_wireless(&content))
    }
}

#[async_trait]
impl NetHealthBackend for LiveNetHealthBackend {
    async fn probe(&self, target: &events::NetHealthTarget, probe_timeout: Duration) -> io::Result<Duration> {
        let start_time = Instant::now();
        let socket_address = format!("{}:{}", target.host, target.port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "no socket address resolved"))?;

        timeout(probe_timeout, TcpStream::connect(socket_address))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "probe timeout"))?
            .map(|_| start_time.elapsed())
    }
}

pub struct NetToolsConfig {
    pulse: Duration,
    hostname: bool,
    routes: bool,
    default_routes: bool,
    nethealth: bool,
    sockets: bool,
    neighbours: bool,
    route_lookups: bool,
    throughput: bool,
    wifi: bool,
    nethealth_window: usize,
    nethealth_timeout: Duration,
    nethealth_latency_degraded_ms: u64,
    nethealth_loss_degraded_pct: u8,
}

impl Default for NetToolsConfig {
    fn default() -> Self {
        Self {
            pulse: Duration::from_secs(3),
            hostname: true,
            routes: true,
            default_routes: true,
            nethealth: false,
            sockets: false,
            neighbours: false,
            route_lookups: false,
            throughput: false,
            wifi: false,
            nethealth_window: 4,
            nethealth_timeout: Duration::from_secs(2),
            nethealth_latency_degraded_ms: 400,
            nethealth_loss_degraded_pct: 25,
        }
    }
}

impl NetToolsConfig {
    pub fn pulse(mut self, pulse: Duration) -> Self {
        self.pulse = pulse;
        self
    }

    fn get_pulse(&self) -> Duration {
        self.pulse
    }

    pub fn hostname(mut self, hostname: bool) -> Self {
        self.hostname = hostname;
        self
    }

    pub fn routes(mut self, routes: bool) -> Self {
        self.routes = routes;
        self
    }

    pub fn default_routes(mut self, default_routes: bool) -> Self {
        self.default_routes = default_routes;
        self
    }

    pub fn nethealth(mut self, nethealth: bool) -> Self {
        self.nethealth = nethealth;
        self
    }

    pub fn sockets(mut self, sockets: bool) -> Self {
        self.sockets = sockets;
        self
    }

    pub fn neighbours(mut self, neighbours: bool) -> Self {
        self.neighbours = neighbours;
        self
    }

    pub fn route_lookups(mut self, route_lookups: bool) -> Self {
        self.route_lookups = route_lookups;
        self
    }

    pub fn throughput(mut self, throughput: bool) -> Self {
        self.throughput = throughput;
        self
    }

    pub fn wifi(mut self, wifi: bool) -> Self {
        self.wifi = wifi;
        self
    }

    pub fn nethealth_window(mut self, nethealth_window: usize) -> Self {
        self.nethealth_window = nethealth_window.max(1);
        self
    }

    pub fn nethealth_timeout(mut self, nethealth_timeout: Duration) -> Self {
        self.nethealth_timeout = nethealth_timeout;
        self
    }

    pub fn nethealth_latency_degraded_ms(mut self, nethealth_latency_degraded_ms: u64) -> Self {
        self.nethealth_latency_degraded_ms = nethealth_latency_degraded_ms;
        self
    }

    pub fn nethealth_loss_degraded_pct(mut self, nethealth_loss_degraded_pct: u8) -> Self {
        self.nethealth_loss_degraded_pct = nethealth_loss_degraded_pct;
        self
    }
}

pub struct NetTools {
    cfg: NetToolsConfig,
    hostname_backend: Arc<dyn HostnameBackend>,
    route_backend: Arc<dyn RouteBackend>,
    nethealth_backend: Arc<dyn NetHealthBackend>,
    socket_backend: Arc<dyn SocketBackend>,
    neighbour_backend: Arc<dyn NeighbourBackend>,
    throughput_backend: Arc<dyn ThroughputBackend>,
    wifi_backend: Arc<dyn WifiBackend>,
    last_hostname: Option<String>,
    last_routes: HashMap<RouteKey, events::RouteEntry>,
    nethealth_targets: Vec<events::NetHealthTarget>,
    nethealth_samples: VecDeque<NetHealthSample>,
    last_nethealth: Option<events::NetHealthState>,
    last_sockets: HashSet<events::SocketEntry>,
    last_neighbours: HashMap<String, events::NeighbourEntry>,
    route_lookup_targets: Vec<String>,
    last_route_lookups: HashMap<RouteLookupKey, events::RouteLookupEntry>,
    last_throughput: Option<ThroughputState>,
    last_wifi: HashMap<String, events::WifiDetails>,
}

impl Default for NetTools {
    fn default() -> Self {
        Self::new(None)
    }
}

impl NetTools {
    pub fn new(cfg: Option<NetToolsConfig>) -> Self {
        Self {
            cfg: cfg.unwrap_or_default(),
            hostname_backend: Arc::new(LiveHostnameBackend),
            route_backend: Arc::new(LiveRouteBackend),
            nethealth_backend: Arc::new(LiveNetHealthBackend),
            socket_backend: Arc::new(LiveSocketBackend),
            neighbour_backend: Arc::new(LiveNeighbourBackend),
            throughput_backend: Arc::new(LiveThroughputBackend),
            wifi_backend: Arc::new(LiveWifiBackend),
            last_hostname: None,
            last_routes: HashMap::new(),
            nethealth_targets: Vec::new(),
            nethealth_samples: VecDeque::new(),
            last_nethealth: None,
            last_sockets: HashSet::new(),
            last_neighbours: HashMap::new(),
            route_lookup_targets: Vec::new(),
            last_route_lookups: HashMap::new(),
            last_throughput: None,
            last_wifi: HashMap::new(),
        }
    }

    pub fn set_hostname_backend<B>(&mut self, backend: B)
    where
        B: HostnameBackend + 'static,
    {
        self.hostname_backend = Arc::new(backend);
    }

    pub fn set_route_backend<B>(&mut self, backend: B)
    where
        B: RouteBackend + 'static,
    {
        self.route_backend = Arc::new(backend);
    }

    pub fn set_nethealth_backend<B>(&mut self, backend: B)
    where
        B: NetHealthBackend + 'static,
    {
        self.nethealth_backend = Arc::new(backend);
    }

    pub fn set_socket_backend<B>(&mut self, backend: B)
    where
        B: SocketBackend + 'static,
    {
        self.socket_backend = Arc::new(backend);
    }

    pub fn set_neighbour_backend<B>(&mut self, backend: B)
    where
        B: NeighbourBackend + 'static,
    {
        self.neighbour_backend = Arc::new(backend);
    }

    pub fn set_throughput_backend<B>(&mut self, backend: B)
    where
        B: ThroughputBackend + 'static,
    {
        self.throughput_backend = Arc::new(backend);
    }

    pub fn set_wifi_backend<B>(&mut self, backend: B)
    where
        B: WifiBackend + 'static,
    {
        self.wifi_backend = Arc::new(backend);
    }

    pub fn add_nethealth_target<S>(&mut self, host: S, port: u16)
    where
        S: Into<String>,
    {
        self.nethealth_targets.push(events::NetHealthTarget {
            host: host.into(),
            port,
        });
    }

    pub fn add_route_lookup_target<S>(&mut self, target: S)
    where
        S: Into<String>,
    {
        self.route_lookup_targets.push(target.into());
    }

    async fn fire(hub: &CallbackHub<NetToolsEvent>, ev: NetToolsEvent) {
        hub.fire(ev.mask().bits(), &ev).await;
    }

    fn poll_hostname(&self) -> io::Result<String> {
        self.hostname_backend.current()
    }

    fn poll_routes(&self) -> io::Result<Vec<events::RouteEntry>> {
        self.route_backend.list()
    }

    fn poll_sockets(&self) -> io::Result<HashSet<events::SocketEntry>> {
        self.socket_backend.list()
    }

    fn poll_neighbours(&self) -> io::Result<HashMap<String, events::NeighbourEntry>> {
        self.neighbour_backend.list()
    }

    fn poll_throughput(&self) -> io::Result<HashMap<String, events::InterfaceCounters>> {
        self.throughput_backend.list()
    }

    fn poll_wifi(&self) -> io::Result<HashMap<String, events::WifiDetails>> {
        self.wifi_backend.list()
    }

    fn store_hostname(&mut self, hostname: String) {
        self.last_hostname = Some(hostname);
    }

    fn route_key(route: &events::RouteEntry) -> RouteKey {
        RouteKey { family: route.family.clone(), destination: route.destination.clone() }
    }

    fn route_map(routes: Vec<events::RouteEntry>) -> HashMap<RouteKey, events::RouteEntry> {
        routes.into_iter().map(|route| (Self::route_key(&route), route)).collect()
    }

    fn route_lookup_key(lookup: &events::RouteLookupEntry) -> RouteLookupKey {
        RouteLookupKey {
            target: lookup.target.clone(),
        }
    }

    fn is_default_route(route: &events::RouteEntry) -> bool {
        matches!(
            route.destination.as_str(),
            "default" | "0.0.0.0/0" | "0.0.0.0" | "::/0"
        )
    }

    fn default_route(routes: &HashMap<RouteKey, events::RouteEntry>) -> Option<events::RouteEntry> {
        routes.values().find(|route| Self::is_default_route(route)).cloned()
    }

    fn parse_target(value: &str) -> Option<std::net::IpAddr> {
        value.parse::<std::net::IpAddr>().ok()
    }

    fn route_prefix_len(route: &events::RouteEntry) -> Option<u8> {
        match route.destination.as_str() {
            "default" => Some(0),
            "0.0.0.0" => Some(0),
            "0.0.0.0/0" => Some(0),
            "::/0" => Some(0),
            destination => destination
                .split_once('/')
                .and_then(|(_, prefix)| prefix.parse::<u8>().ok())
                .or_else(|| Self::parse_target(destination).map(|ip| if ip.is_ipv4() { 32 } else { 128 })),
        }
    }

    fn route_matches_target(route: &events::RouteEntry, target: std::net::IpAddr) -> bool {
        match (target, route.destination.as_str()) {
            (std::net::IpAddr::V4(_), "default" | "0.0.0.0" | "0.0.0.0/0") => true,
            (std::net::IpAddr::V6(_), "::/0") => true,
            (std::net::IpAddr::V4(target), destination) => destination
                .split_once('/')
                .and_then(|(network, prefix)| {
                    let network = network.parse::<std::net::Ipv4Addr>().ok()?;
                    let prefix = prefix.parse::<u32>().ok()?;
                    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
                    Some((u32::from(target) & mask) == (u32::from(network) & mask))
                })
                .or_else(|| destination.parse::<std::net::Ipv4Addr>().ok().map(|network| network == target))
                .unwrap_or(false),
            (std::net::IpAddr::V6(target), destination) => destination
                .split_once('/')
                .and_then(|(network, prefix)| {
                    let network = network.parse::<std::net::Ipv6Addr>().ok()?;
                    let prefix = prefix.parse::<u32>().ok()?;
                    let target = u128::from(target);
                    let network = u128::from(network);
                    let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
                    Some((target & mask) == (network & mask))
                })
                .or_else(|| destination.parse::<std::net::Ipv6Addr>().ok().map(|network| network == target))
                .unwrap_or(false),
        }
    }

    fn route_lookup(routes: &HashMap<RouteKey, events::RouteEntry>, target: &str) -> Option<events::RouteLookupEntry> {
        let target_ip = Self::parse_target(target)?;
        routes
            .values()
            .filter(|route| {
                matches!(
                    (&target_ip, &route.family),
                    (std::net::IpAddr::V4(_), events::RouteFamily::Inet)
                        | (std::net::IpAddr::V6(_), events::RouteFamily::Inet6)
                        | (_, events::RouteFamily::Unknown)
                ) && Self::route_matches_target(route, target_ip)
            })
            .max_by_key(|route| Self::route_prefix_len(route).unwrap_or(0))
            .cloned()
            .map(|route| events::RouteLookupEntry {
                target: target.to_string(),
                route,
            })
    }

    fn route_lookup_map(
        routes: &HashMap<RouteKey, events::RouteEntry>,
        targets: &[String],
    ) -> HashMap<RouteLookupKey, events::RouteLookupEntry> {
        targets
            .iter()
            .filter_map(|target| Self::route_lookup(routes, target))
            .map(|lookup| (Self::route_lookup_key(&lookup), lookup))
            .collect()
    }

    fn trim_nethealth_samples(&mut self) {
        while self.nethealth_samples.len() > self.cfg.nethealth_window {
            self.nethealth_samples.pop_front();
        }
    }

    fn throughput_sample(
        previous: &events::InterfaceCounters,
        current: &events::InterfaceCounters,
        interval: Duration,
    ) -> Option<events::ThroughputSample> {
        (interval.as_millis() > 0).then(|| events::ThroughputSample {
            iface: current.iface.clone(),
            interval_ms: interval.as_millis() as u64,
            rx_bytes_per_sec: current
                .rx_bytes
                .saturating_sub(previous.rx_bytes)
                .saturating_mul(1000)
                / interval.as_millis() as u64,
            tx_bytes_per_sec: current
                .tx_bytes
                .saturating_sub(previous.tx_bytes)
                .saturating_mul(1000)
                / interval.as_millis() as u64,
            rx_packets_per_sec: current
                .rx_packets
                .saturating_sub(previous.rx_packets)
                .saturating_mul(1000)
                / interval.as_millis() as u64,
            tx_packets_per_sec: current
                .tx_packets
                .saturating_sub(previous.tx_packets)
                .saturating_mul(1000)
                / interval.as_millis() as u64,
            counters: current.clone(),
        })
    }

    fn nethealth_state(&self) -> Option<events::NetHealthState> {
        if self.nethealth_samples.is_empty() {
            return None;
        }

        let (total_probes, successful_probes, latency_sum_ms) = self.nethealth_samples.iter().fold(
            (0usize, 0usize, 0u64),
            |(total_probes, successful_probes, latency_sum_ms), sample| {
                (
                    total_probes + sample.total_probes,
                    successful_probes + sample.successful_probes,
                    latency_sum_ms + sample.latency_sum_ms,
                )
            },
        );

        if total_probes == 0 {
            return None;
        }

        let loss_pct = (((total_probes - successful_probes) * 100) / total_probes) as u8;
        let avg_rtt_ms = if successful_probes > 0 {
            Some(latency_sum_ms / successful_probes as u64)
        } else {
            None
        };
        let level = if successful_probes == 0 {
            events::NetHealthLevel::Down
        } else if loss_pct >= self.cfg.nethealth_loss_degraded_pct
            || avg_rtt_ms.is_some_and(|avg_rtt_ms| avg_rtt_ms >= self.cfg.nethealth_latency_degraded_ms)
        {
            events::NetHealthLevel::Degraded
        } else {
            events::NetHealthLevel::Healthy
        };

        Some(events::NetHealthState {
            level,
            avg_rtt_ms,
            loss_pct,
            successful_probes,
            total_probes,
        })
    }

    async fn nethealth_sample(&self) -> NetHealthSample {
        let mut total_probes = 0usize;
        let mut successful_probes = 0usize;
        let mut latency_sum_ms = 0u64;

        for target in &self.nethealth_targets {
            total_probes += 1;
            if let Ok(duration) = self
                .nethealth_backend
                .probe(target, self.cfg.nethealth_timeout)
                .await
            {
                successful_probes += 1;
                latency_sum_ms += duration.as_millis() as u64;
            }
        }

        NetHealthSample {
            total_probes,
            successful_probes,
            latency_sum_ms,
        }
    }

    async fn handle_nethealth_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        if self.nethealth_targets.is_empty() {
            return;
        }

        self.nethealth_samples.push_back(self.nethealth_sample().await);
        self.trim_nethealth_samples();

        if let Some(current_nethealth) = self.nethealth_state() {
            if let Some(previous_nethealth) = self.last_nethealth.as_ref()
                && previous_nethealth != &current_nethealth
            {
                Self::fire(
                    hub,
                    NetToolsEvent::NetHealthChanged {
                        old: previous_nethealth.clone(),
                        new: current_nethealth.clone(),
                    },
                )
                .await;
            }

            self.last_nethealth = Some(current_nethealth);
        }
    }

    async fn handle_socket_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        let current_sockets = match self.poll_sockets() {
            Ok(current_sockets) => current_sockets,
            Err(err) => {
                log::error!("nettools: failed to read sockets: {err}");
                return;
            }
        };

        for socket in current_sockets.difference(&self.last_sockets) {
            Self::fire(
                hub,
                NetToolsEvent::SocketAdded {
                    socket: socket.clone(),
                },
            )
            .await;
        }

        for socket in self.last_sockets.difference(&current_sockets) {
            Self::fire(
                hub,
                NetToolsEvent::SocketRemoved {
                    socket: socket.clone(),
                },
            )
            .await;
        }

        self.last_sockets = current_sockets;
    }

    async fn handle_neighbour_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        let current_neighbours = match self.poll_neighbours() {
            Ok(current_neighbours) => current_neighbours,
            Err(err) => {
                log::error!("nettools: failed to read neighbours: {err}");
                return;
            }
        };

        for (address, current_neighbour) in &current_neighbours {
            if let Some(previous_neighbour) = self.last_neighbours.get(address) {
                if previous_neighbour != current_neighbour {
                    Self::fire(
                        hub,
                        NetToolsEvent::NeighbourChanged {
                            old: previous_neighbour.clone(),
                            new: current_neighbour.clone(),
                        },
                    )
                    .await;
                }
            } else {
                Self::fire(
                    hub,
                    NetToolsEvent::NeighbourAdded {
                        neighbour: current_neighbour.clone(),
                    },
                )
                .await;
            }
        }

        for (address, previous_neighbour) in &self.last_neighbours {
            if !current_neighbours.contains_key(address) {
                Self::fire(
                    hub,
                    NetToolsEvent::NeighbourRemoved {
                        neighbour: previous_neighbour.clone(),
                    },
                )
                .await;
            }
        }

        self.last_neighbours = current_neighbours;
    }

    async fn handle_throughput_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        let current_state = match self.poll_throughput() {
            Ok(counters) => ThroughputState {
                at: Instant::now(),
                counters,
            },
            Err(err) => {
                log::error!("nettools: failed to read interface counters: {err}");
                return;
            }
        };

        if let Some(previous_state) = self.last_throughput.as_ref() {
            for sample in current_state
                .counters
                .iter()
                .filter_map(|(iface, current)| {
                    previous_state.counters.get(iface).and_then(|previous| {
                        Self::throughput_sample(previous, current, current_state.at.saturating_duration_since(previous_state.at))
                    })
                })
                .filter(|sample| {
                    sample.rx_bytes_per_sec > 0
                        || sample.tx_bytes_per_sec > 0
                        || sample.rx_packets_per_sec > 0
                        || sample.tx_packets_per_sec > 0
                })
            {
                Self::fire(hub, NetToolsEvent::ThroughputUpdated { sample }).await;
            }
        }

        self.last_throughput = Some(current_state);
    }

    async fn handle_wifi_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        let current_wifi = match self.poll_wifi() {
            Ok(current_wifi) => current_wifi,
            Err(err) => {
                log::error!("nettools: failed to read wifi details: {err}");
                return;
            }
        };

        for (iface, current) in &current_wifi {
            if let Some(previous) = self.last_wifi.get(iface) {
                if previous != current {
                    Self::fire(
                        hub,
                        NetToolsEvent::WifiChanged {
                            old: previous.clone(),
                            new: current.clone(),
                        },
                    )
                    .await;
                }
            } else {
                Self::fire(hub, NetToolsEvent::WifiAdded { wifi: current.clone() }).await;
            }
        }

        for (iface, previous) in &self.last_wifi {
            if !current_wifi.contains_key(iface) {
                Self::fire(hub, NetToolsEvent::WifiRemoved { wifi: previous.clone() }).await;
            }
        }

        self.last_wifi = current_wifi;
    }

    async fn handle_hostname_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        let current_hostname = match self.poll_hostname() {
            Ok(current_hostname) => current_hostname,
            Err(err) => {
                log::error!("nettools: failed to read hostname: {err}");
                return;
            }
        };

        if let Some(previous_hostname) = self.last_hostname.as_ref()
            && previous_hostname != &current_hostname
        {
            Self::fire(
                hub,
                NetToolsEvent::HostnameChanged { old: previous_hostname.clone(), new: current_hostname.clone() },
            )
            .await;
        }

        self.store_hostname(current_hostname);
    }

    async fn handle_route_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        let current_routes = match self.poll_routes() {
            Ok(current_routes) => Self::route_map(current_routes),
            Err(err) => {
                log::error!("nettools: failed to read routes: {err}");
                return;
            }
        };
        let previous_default_route = self.cfg.default_routes.then(|| Self::default_route(&self.last_routes)).flatten();
        let current_default_route = self.cfg.default_routes.then(|| Self::default_route(&current_routes)).flatten();
        let current_route_lookups = if self
            .cfg
            .route_lookups { Self::route_lookup_map(&current_routes, &self.route_lookup_targets) } else { Default::default() };

        if self.cfg.routes {
            for (route_key, current_route) in &current_routes {
                if let Some(previous_route) = self.last_routes.get(route_key) {
                    if previous_route != current_route {
                        Self::fire(
                            hub,
                            NetToolsEvent::RouteChanged {
                                old: previous_route.clone(),
                                new: current_route.clone(),
                            },
                        )
                        .await;
                    }
                } else {
                    Self::fire(
                        hub,
                        NetToolsEvent::RouteAdded {
                            route: current_route.clone(),
                        },
                    )
                    .await;
                }
            }

            for (route_key, previous_route) in &self.last_routes {
                if !current_routes.contains_key(route_key) {
                    Self::fire(
                        hub,
                        NetToolsEvent::RouteRemoved {
                            route: previous_route.clone(),
                        },
                    )
                    .await;
                }
            }
        }

        if let Some(current_default_route) = current_default_route.as_ref() {
            if let Some(previous_default_route) = previous_default_route.as_ref() {
                if previous_default_route != current_default_route {
                    Self::fire(
                        hub,
                        NetToolsEvent::DefaultRouteChanged {
                            old: previous_default_route.clone(),
                            new: current_default_route.clone(),
                        },
                    )
                    .await;
                }
            } else {
                Self::fire(hub, NetToolsEvent::DefaultRouteAdded { route: current_default_route.clone() }).await;
            }
        } else if let Some(previous_default_route) = previous_default_route.as_ref() {
            Self::fire(hub, NetToolsEvent::DefaultRouteRemoved { route: previous_default_route.clone() }).await;
        }

        if self.cfg.route_lookups {
            for (lookup_key, current_lookup) in &current_route_lookups {
                if let Some(previous_lookup) = self.last_route_lookups.get(lookup_key) {
                    if previous_lookup != current_lookup {
                        Self::fire(
                            hub,
                            NetToolsEvent::RouteLookupChanged {
                                old: previous_lookup.clone(),
                                new: current_lookup.clone(),
                            },
                        )
                        .await;
                    }
                } else {
                    Self::fire(
                        hub,
                        NetToolsEvent::RouteLookupAdded {
                            lookup: current_lookup.clone(),
                        },
                    )
                    .await;
                }
            }

            for (lookup_key, previous_lookup) in &self.last_route_lookups {
                if !current_route_lookups.contains_key(lookup_key) {
                    Self::fire(
                        hub,
                        NetToolsEvent::RouteLookupRemoved {
                            lookup: previous_lookup.clone(),
                        },
                    )
                    .await;
                }
            }
        }

        self.last_routes = current_routes;
        self.last_route_lookups = current_route_lookups;
    }

    pub async fn run(mut self, ctx: SensorCtx<NetToolsEvent>) {
        if self.cfg.hostname {
            self.handle_hostname_poll(&ctx.hub).await;
        }

        if self.cfg.routes || self.cfg.default_routes || self.cfg.route_lookups {
            self.last_routes = Self::route_map(self.poll_routes().unwrap_or_default());
            self.last_route_lookups = Self::route_lookup_map(&self.last_routes, &self.route_lookup_targets);
        }

        if self.cfg.nethealth {
            self.last_nethealth = None;
        }

        if self.cfg.sockets {
            self.last_sockets = self.poll_sockets().unwrap_or_default();
        }

        if self.cfg.neighbours {
            self.last_neighbours = self.poll_neighbours().unwrap_or_default();
        }

        if self.cfg.throughput {
            self.last_throughput = self.poll_throughput().ok().map(|counters| ThroughputState {
                at: Instant::now(),
                counters,
            });
        }

        if self.cfg.wifi {
            self.last_wifi = self.poll_wifi().unwrap_or_default();
        }

        let mut ticker = tokio::time::interval(self.cfg.get_pulse());

        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                _ = ticker.tick() => {
                    if self.cfg.hostname {
                        self.handle_hostname_poll(&ctx.hub).await;
                    }

                    if self.cfg.routes || self.cfg.default_routes || self.cfg.route_lookups {
                        self.handle_route_poll(&ctx.hub).await;
                    }

                    if self.cfg.nethealth {
                        self.handle_nethealth_poll(&ctx.hub).await;
                    }

                    if self.cfg.sockets {
                        self.handle_socket_poll(&ctx.hub).await;
                    }

                    if self.cfg.neighbours {
                        self.handle_neighbour_poll(&ctx.hub).await;
                    }

                    if self.cfg.throughput {
                        self.handle_throughput_poll(&ctx.hub).await;
                    }

                    if self.cfg.wifi {
                        self.handle_wifi_poll(&ctx.hub).await;
                    }
                },
            }
        }
    }
}

impl Sensor for NetTools {
    type Event = NetToolsEvent;

    fn run(self, ctx: SensorCtx<Self::Event>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { self.run(ctx).await })
    }
}
