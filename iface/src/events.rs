use bitflags::bitflags;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IfaceEvent {
    IfaceAdded { ifindex: u32, ifname: String },
    IfaceRemoved { ifindex: u32, ifname: String },
    LinkUp { ifindex: u32, ifname: String },
    LinkDown { ifindex: u32, ifname: String },
    AddrAdded { ifindex: u32, ifname: String },
    AddrRemoved { ifindex: u32, ifname: String },
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct IfaceMask: u64 {
        const IFACE_ADDED   = 0b000001;
        const IFACE_REMOVED = 0b000010;
        const LINK_UP       = 0b000100;
        const LINK_DOWN     = 0b001000;
        const ADDR_ADDED    = 0b010000;
        const ADDR_REMOVED  = 0b100000;
    }
}

impl IfaceEvent {
    pub fn mask(&self) -> IfaceMask {
        match self {
            IfaceEvent::IfaceAdded { .. } => IfaceMask::IFACE_ADDED,
            IfaceEvent::IfaceRemoved { .. } => IfaceMask::IFACE_REMOVED,
            IfaceEvent::LinkUp { .. } => IfaceMask::LINK_UP,
            IfaceEvent::LinkDown { .. } => IfaceMask::LINK_DOWN,
            IfaceEvent::AddrAdded { .. } => IfaceMask::ADDR_ADDED,
            IfaceEvent::AddrRemoved { .. } => IfaceMask::ADDR_REMOVED,
        }
    }
}
