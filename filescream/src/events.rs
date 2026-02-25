use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum FileScreamEvent {
    Created { path: PathBuf },
    Changed { path: PathBuf },
    Removed { path: PathBuf },
}
bitflags::bitflags! {
    #[derive(Copy, Clone)]
    pub struct EventMask: u8 {
        const CREATED = 0b0001;
        const CHANGED = 0b0010;
        const REMOVED = 0b0100;
    }
}
impl EventMask {
    pub fn matches(&self, ev: &FileScreamEvent) -> bool {
        match ev {
            FileScreamEvent::Created { .. } => self.contains(EventMask::CREATED),
            FileScreamEvent::Changed { .. } => self.contains(EventMask::CHANGED),
            FileScreamEvent::Removed { .. } => self.contains(EventMask::REMOVED),
        }
    }
}

pub type CallbackResult = serde_json::Value;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait FileScreamCallback: Send + Sync + 'static {
    fn mask(&self) -> EventMask;
    fn call<'a>(&'a self, ev: &'a FileScreamEvent) -> BoxFuture<'a, Option<CallbackResult>>;
}

#[allow(clippy::type_complexity)]
pub struct Callback {
    mask: EventMask,
    handlers: Vec<Arc<dyn Fn(FileScreamEvent) -> BoxFuture<'static, Option<CallbackResult>> + Send + Sync>>,
}

impl Callback {
    pub fn new(mask: EventMask) -> Self {
        Self { mask, handlers: Vec::new() }
    }

    pub fn on<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(FileScreamEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<CallbackResult>> + Send + 'static,
    {
        self.handlers.push(Arc::new(move |ev| Box::pin(f(ev))));
        self
    }
}

impl FileScreamCallback for Callback {
    fn mask(&self) -> EventMask {
        self.mask
    }

    fn call<'a>(&'a self, ev: &'a FileScreamEvent) -> BoxFuture<'a, Option<CallbackResult>> {
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
