use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum ProcDogEvent {
    Appeared { name: String, pid: i32 },
    Disappeared { name: String, pid: i32 },
    Missing { name: String },
}

bitflags::bitflags! {
    #[derive(Copy, Clone)]
    pub struct EventMask: u8 {
        const APPEARED    = 0b0001;
        const DISAPPEARED = 0b0010;
        const MISSING     = 0b0100;
    }
}

impl EventMask {
    pub fn matches(&self, ev: &ProcDogEvent) -> bool {
        match ev {
            ProcDogEvent::Appeared { .. } => self.contains(EventMask::APPEARED),
            ProcDogEvent::Disappeared { .. } => self.contains(EventMask::DISAPPEARED),
            ProcDogEvent::Missing { .. } => self.contains(EventMask::MISSING),
        }
    }
}

pub type CallbackResult = serde_json::Value;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ProcDogCallback: Send + Sync + 'static {
    fn mask(&self) -> EventMask;
    fn call<'a>(&'a self, ev: &'a ProcDogEvent) -> BoxFuture<'a, Option<CallbackResult>>;
}

#[allow(clippy::type_complexity)]
pub struct Callback {
    mask: EventMask,
    handlers:
        Vec<Arc<dyn Fn(ProcDogEvent) -> BoxFuture<'static, Option<CallbackResult>> + Send + Sync>>,
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
        F: Fn(ProcDogEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<CallbackResult>> + Send + 'static,
    {
        self.handlers.push(Arc::new(move |ev| Box::pin(f(ev))));
        self
    }
}

impl ProcDogCallback for Callback {
    fn mask(&self) -> EventMask {
        self.mask
    }

    fn call<'a>(&'a self, ev: &'a ProcDogEvent) -> BoxFuture<'a, Option<CallbackResult>> {
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
