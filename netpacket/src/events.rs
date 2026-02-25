use bitflags::bitflags;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnKey {
    pub proto: String, // "tcp","udp","tcp6","udp6"

    // Raw data
    pub local: String,         // "ip:port"
    pub remote: String,        // "ip:port"
    pub state: Option<String>, // tcp state; udp None

    // decoded (best-effort)
    pub local_dec: Option<String>,  // "192.168.2.136:57843"
    pub remote_dec: Option<String>, // "172.64.155.209:443"
    pub state_dec: Option<String>,  // "ESTABLISHED" etc (tcp only)

    pub local_host: Option<String>,
    pub remote_host: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetNotifyEvent {
    Opened { conn: ConnKey },
    Closed { conn: ConnKey },
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct NetNotifyMask: u64 {
        const OPENED = 0b0001;
        const CLOSED = 0b0010;
    }
}

impl NetNotifyEvent {
    pub fn mask(&self) -> NetNotifyMask {
        match self {
            NetNotifyEvent::Opened { .. } => NetNotifyMask::OPENED,
            NetNotifyEvent::Closed { .. } => NetNotifyMask::CLOSED,
        }
    }
}
