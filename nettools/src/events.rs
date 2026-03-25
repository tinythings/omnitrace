use bitflags::bitflags;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteFamily {
    Inet,
    Inet6,
    Unknown,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteEntry {
    pub family: RouteFamily,
    pub destination: String,
    pub gateway: String,
    pub iface: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetHealthLevel {
    Healthy,
    Degraded,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetHealthTarget {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetHealthState {
    pub level: NetHealthLevel,
    pub avg_rtt_ms: Option<u64>,
    pub loss_pct: u8,
    pub successful_probes: usize,
    pub total_probes: usize,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocketKind {
    Listener,
    Connection,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocketEntry {
    pub proto: String,
    pub local: String,
    pub remote: String,
    pub state: Option<String>,
    pub kind: SocketKind,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighbourEntry {
    pub address: String,
    pub mac: String,
    pub iface: String,
    pub state: Option<String>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteLookupEntry {
    pub target: String,
    pub route: RouteEntry,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceCounters {
    pub iface: String,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errors: u64,
    pub rx_drops: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errors: u64,
    pub tx_drops: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThroughputSample {
    pub iface: String,
    pub interval_ms: u64,
    pub rx_bytes_per_sec: u64,
    pub tx_bytes_per_sec: u64,
    pub rx_packets_per_sec: u64,
    pub tx_packets_per_sec: u64,
    pub counters: InterfaceCounters,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WifiDetails {
    pub iface: String,
    pub connected: bool,
    pub link_quality: f32,
    pub signal_level_dbm: f32,
    pub noise_level_dbm: f32,
    pub ssid: Option<String>,
    pub bssid: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetToolsEvent {
    HostnameChanged { old: String, new: String },
    RouteAdded { route: RouteEntry },
    RouteRemoved { route: RouteEntry },
    RouteChanged { old: RouteEntry, new: RouteEntry },
    DefaultRouteAdded { route: RouteEntry },
    DefaultRouteRemoved { route: RouteEntry },
    DefaultRouteChanged { old: RouteEntry, new: RouteEntry },
    NetHealthChanged { old: NetHealthState, new: NetHealthState },
    SocketAdded { socket: SocketEntry },
    SocketRemoved { socket: SocketEntry },
    NeighbourAdded { neighbour: NeighbourEntry },
    NeighbourRemoved { neighbour: NeighbourEntry },
    NeighbourChanged { old: NeighbourEntry, new: NeighbourEntry },
    RouteLookupAdded { lookup: RouteLookupEntry },
    RouteLookupRemoved { lookup: RouteLookupEntry },
    RouteLookupChanged { old: RouteLookupEntry, new: RouteLookupEntry },
    ThroughputUpdated { sample: ThroughputSample },
    WifiAdded { wifi: WifiDetails },
    WifiRemoved { wifi: WifiDetails },
    WifiChanged { old: WifiDetails, new: WifiDetails },
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct NetToolsMask: u64 {
        const HOSTNAME_CHANGED      = 0b0000001;
        const ROUTE_ADDED           = 0b0000010;
        const ROUTE_REMOVED         = 0b0000100;
        const ROUTE_CHANGED         = 0b0001000;
        const DEFAULT_ROUTE_ADDED   = 0b0010000;
        const DEFAULT_ROUTE_REMOVED = 0b0100000;
        const DEFAULT_ROUTE_CHANGED = 0b1000000;
        const NETHEALTH_CHANGED     = 0b10000000;
        const SOCKET_ADDED          = 0b100000000;
        const SOCKET_REMOVED        = 0b1000000000;
        const NEIGHBOUR_ADDED       = 0b10000000000;
        const NEIGHBOUR_REMOVED     = 0b100000000000;
        const NEIGHBOUR_CHANGED     = 0b1000000000000;
        const ROUTE_LOOKUP_ADDED    = 0b10000000000000;
        const ROUTE_LOOKUP_REMOVED  = 0b100000000000000;
        const ROUTE_LOOKUP_CHANGED  = 0b1000000000000000;
        const THROUGHPUT_UPDATED    = 0b10000000000000000;
        const WIFI_ADDED            = 0b100000000000000000;
        const WIFI_REMOVED          = 0b1000000000000000000;
        const WIFI_CHANGED          = 0b10000000000000000000;
    }
}

impl NetToolsEvent {
    pub fn mask(&self) -> NetToolsMask {
        match self {
            NetToolsEvent::HostnameChanged { .. } => NetToolsMask::HOSTNAME_CHANGED,
            NetToolsEvent::RouteAdded { .. } => NetToolsMask::ROUTE_ADDED,
            NetToolsEvent::RouteRemoved { .. } => NetToolsMask::ROUTE_REMOVED,
            NetToolsEvent::RouteChanged { .. } => NetToolsMask::ROUTE_CHANGED,
            NetToolsEvent::DefaultRouteAdded { .. } => NetToolsMask::DEFAULT_ROUTE_ADDED,
            NetToolsEvent::DefaultRouteRemoved { .. } => NetToolsMask::DEFAULT_ROUTE_REMOVED,
            NetToolsEvent::DefaultRouteChanged { .. } => NetToolsMask::DEFAULT_ROUTE_CHANGED,
            NetToolsEvent::NetHealthChanged { .. } => NetToolsMask::NETHEALTH_CHANGED,
            NetToolsEvent::SocketAdded { .. } => NetToolsMask::SOCKET_ADDED,
            NetToolsEvent::SocketRemoved { .. } => NetToolsMask::SOCKET_REMOVED,
            NetToolsEvent::NeighbourAdded { .. } => NetToolsMask::NEIGHBOUR_ADDED,
            NetToolsEvent::NeighbourRemoved { .. } => NetToolsMask::NEIGHBOUR_REMOVED,
            NetToolsEvent::NeighbourChanged { .. } => NetToolsMask::NEIGHBOUR_CHANGED,
            NetToolsEvent::RouteLookupAdded { .. } => NetToolsMask::ROUTE_LOOKUP_ADDED,
            NetToolsEvent::RouteLookupRemoved { .. } => NetToolsMask::ROUTE_LOOKUP_REMOVED,
            NetToolsEvent::RouteLookupChanged { .. } => NetToolsMask::ROUTE_LOOKUP_CHANGED,
            NetToolsEvent::ThroughputUpdated { .. } => NetToolsMask::THROUGHPUT_UPDATED,
            NetToolsEvent::WifiAdded { .. } => NetToolsMask::WIFI_ADDED,
            NetToolsEvent::WifiRemoved { .. } => NetToolsMask::WIFI_REMOVED,
            NetToolsEvent::WifiChanged { .. } => NetToolsMask::WIFI_CHANGED,
        }
    }
}
