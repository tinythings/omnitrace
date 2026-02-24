use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

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

bitflags::bitflags! {
    #[derive(Copy, Clone)]
    pub struct EventMask: u8 {
        const MOUNTED   = 0b0001;
        const UNMOUNTED = 0b0010;
        const CHANGED   = 0b0100;
    }
}

impl EventMask {
    pub fn matches(&self, ev: &XMountEvent) -> bool {
        match ev {
            XMountEvent::Mounted { .. } => self.contains(EventMask::MOUNTED),
            XMountEvent::Unmounted { .. } => self.contains(EventMask::UNMOUNTED),
            XMountEvent::Changed { .. } => self.contains(EventMask::CHANGED),
        }
    }
}

pub type CallbackResult = serde_json::Value;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait XMountCallback: Send + Sync + 'static {
    fn mask(&self) -> EventMask;
    fn call<'a>(&'a self, ev: &'a XMountEvent) -> BoxFuture<'a, Option<CallbackResult>>;
}

#[allow(clippy::type_complexity)]
pub struct Callback {
    mask: EventMask,
    handlers:
        Vec<Arc<dyn Fn(XMountEvent) -> BoxFuture<'static, Option<CallbackResult>> + Send + Sync>>,
}

impl Callback {
    pub fn new(mask: EventMask) -> Self {
        Self {
            mask,
            handlers: Vec::new(),
        }
    }

    pub fn on<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(XMountEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<CallbackResult>> + Send + 'static,
    {
        self.handlers.push(Arc::new(move |ev| Box::pin(f(ev))));
        self
    }
}

impl XMountCallback for Callback {
    fn mask(&self) -> EventMask {
        self.mask
    }

    fn call<'a>(&'a self, ev: &'a XMountEvent) -> BoxFuture<'a, Option<CallbackResult>> {
        Box::pin(async move {
            for h in &self.handlers {
                if !self.mask.matches(ev) {
                    continue;
                }
                if let Some(r) = h(ev.clone()).await {
                    return Some(r);
                }
            }
            None
        })
    }
}
