use bitflags::bitflags;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct MountInfo {
    pub mount_id: u32,
    pub parent_id: u32,
    pub mount_point: PathBuf,
    pub root: PathBuf,
    pub fstype: String,
    pub source: String,
    pub mount_opts: String,
    pub super_opts: String,
}

#[derive(Clone, Debug)]
pub enum XMountEvent {
    Mounted {
        target: PathBuf,
        info: MountInfo,
    },
    Unmounted {
        target: PathBuf,
        last: MountInfo,
    },
    Changed {
        target: PathBuf,
        old: MountInfo,
        new: MountInfo,
    },
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct XMountMask: u64 {
        const MOUNTED   = 0b0001;
        const UNMOUNTED = 0b0010;
        const CHANGED   = 0b0100;
    }
}

impl XMountEvent {
    pub fn mask(&self) -> XMountMask {
        match self {
            XMountEvent::Mounted { .. } => XMountMask::MOUNTED,
            XMountEvent::Unmounted { .. } => XMountMask::UNMOUNTED,
            XMountEvent::Changed { .. } => XMountMask::CHANGED,
        }
    }
}
