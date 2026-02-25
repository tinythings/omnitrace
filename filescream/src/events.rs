use bitflags::bitflags;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum FileScreamEvent {
    Created { path: PathBuf },
    Changed { path: PathBuf },
    Removed { path: PathBuf },
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct FileScreamMask: u64 {
        const CREATED = 0b0001;
        const CHANGED = 0b0010;
        const REMOVED = 0b0100;
    }
}

impl FileScreamEvent {
    pub fn mask(&self) -> FileScreamMask {
        match self {
            FileScreamEvent::Created { .. } => FileScreamMask::CREATED,
            FileScreamEvent::Changed { .. } => FileScreamMask::CHANGED,
            FileScreamEvent::Removed { .. } => FileScreamMask::REMOVED,
        }
    }
}
