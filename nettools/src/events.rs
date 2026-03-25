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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetToolsEvent {
    HostnameChanged { old: String, new: String },
    RouteAdded { route: RouteEntry },
    RouteRemoved { route: RouteEntry },
    RouteChanged { old: RouteEntry, new: RouteEntry },
    DefaultRouteAdded { route: RouteEntry },
    DefaultRouteRemoved { route: RouteEntry },
    DefaultRouteChanged { old: RouteEntry, new: RouteEntry },
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
        }
    }
}
