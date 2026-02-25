use bitflags::bitflags;

#[derive(Clone, Debug)]
pub enum ProcDogEvent {
    Appeared { name: String, pid: i32 },
    Disappeared { name: String, pid: i32 },
    Missing { name: String },
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct ProcDogMask: u64 {
        const APPEARED    = 0b0001;
        const DISAPPEARED = 0b0010;
        const MISSING     = 0b0100;
    }
}

impl ProcDogEvent {
    pub fn mask(&self) -> ProcDogMask {
        match self {
            ProcDogEvent::Appeared { .. } => ProcDogMask::APPEARED,
            ProcDogEvent::Disappeared { .. } => ProcDogMask::DISAPPEARED,
            ProcDogEvent::Missing { .. } => ProcDogMask::MISSING,
        }
    }
}
