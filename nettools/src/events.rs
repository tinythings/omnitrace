use bitflags::bitflags;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetToolsEvent {
    HostnameChanged { old: String, new: String },
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct NetToolsMask: u64 {
        const HOSTNAME_CHANGED = 0b0001;
    }
}

impl NetToolsEvent {
    pub fn mask(&self) -> NetToolsMask {
        match self {
            NetToolsEvent::HostnameChanged { .. } => NetToolsMask::HOSTNAME_CHANGED,
        }
    }
}
