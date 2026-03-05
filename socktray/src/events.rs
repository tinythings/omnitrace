use bitflags::bitflags;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SockKey {
    pub proto: String,
    pub local: String,
    pub remote: String,
    pub state: Option<String>,
    pub local_dec: Option<String>,
    pub remote_dec: Option<String>,
    pub state_dec: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SockTrayEvent {
    Opened { sock: SockKey },
    Closed { sock: SockKey },
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct SockTrayMask: u64 {
        const OPENED = 0b0001;
        const CLOSED = 0b0010;
    }
}

impl SockTrayEvent {
    pub fn mask(&self) -> SockTrayMask {
        match self {
            SockTrayEvent::Opened { .. } => SockTrayMask::OPENED,
            SockTrayEvent::Closed { .. } => SockTrayMask::CLOSED,
        }
    }
}
