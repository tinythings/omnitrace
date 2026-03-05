pub mod backends;
pub mod events;

use crate::events::IfaceEvent;
use omnitrace_core::{
    callbacks::CallbackHub,
    sensor::{Sensor, SensorCtx},
};
use std::{future::Future, io, pin::Pin, time::Duration};

#[async_trait::async_trait]
pub trait IfaceBackend: Send {
    async fn next_event(&mut self, timeout: Duration) -> io::Result<Option<IfaceEvent>>;
}

pub struct IfaceConfig {
    poll_timeout: Duration,
}

impl Default for IfaceConfig {
    fn default() -> Self {
        Self { poll_timeout: Duration::from_millis(250) }
    }
}

impl IfaceConfig {
    pub fn poll_timeout(mut self, d: Duration) -> Self {
        self.poll_timeout = d;
        self
    }
}

pub struct Iface {
    cfg: IfaceConfig,
    backend: Box<dyn IfaceBackend>,
}

impl Default for Iface {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Iface {
    pub fn new(cfg: Option<IfaceConfig>) -> Self {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let backend: Box<dyn IfaceBackend> = match backends::linux_rtnetlink::LinuxRtNetlinkBackend::new() {
            Ok(b) => Box::new(b),
            Err(e) => {
                log::error!("iface: failed to start rtnetlink backend: {e}");
                Box::new(backends::unsupported::UnsupportedBackend)
            }
        };

        #[cfg(any(target_os = "netbsd", target_os = "freebsd"))]
        let backend: Box<dyn IfaceBackend> = match backends::bsd_route::BsdRouteBackend::new() {
            Ok(b) => Box::new(b),
            Err(e) => {
                log::error!("iface: failed to start PF_ROUTE backend: {e}");
                Box::new(backends::unsupported::UnsupportedBackend)
            }
        };

        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "netbsd", target_os = "freebsd")))]
        let backend: Box<dyn IfaceBackend> = Box::new(backends::unsupported::UnsupportedBackend);

        Self { cfg: cfg.unwrap_or_default(), backend }
    }

    pub fn set_backend<B>(&mut self, backend: B)
    where
        B: IfaceBackend + 'static,
    {
        self.backend = Box::new(backend);
    }

    async fn fire(hub: &CallbackHub<IfaceEvent>, ev: IfaceEvent) {
        hub.fire(ev.mask().bits(), &ev).await;
    }

    pub async fn run(mut self, ctx: SensorCtx<IfaceEvent>) {
        loop {
            if ctx.cancel.is_cancelled() {
                break;
            }

            match self.backend.next_event(self.cfg.poll_timeout).await {
                Ok(Some(ev)) => Self::fire(&ctx.hub, ev).await,
                Ok(None) => {}
                Err(e) => log::error!("iface: backend event read failed: {e}"),
            }
        }
    }
}

impl Sensor for Iface {
    type Event = IfaceEvent;

    fn run(self, ctx: SensorCtx<Self::Event>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { Iface::run(self, ctx).await })
    }
}
