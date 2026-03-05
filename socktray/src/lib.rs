pub mod backends;
pub mod events;

use crate::events::SockTrayEvent;
use glob::Pattern;
use omnitrace_core::{
    callbacks::CallbackHub,
    sensor::{Sensor, SensorCtx},
};
use std::{
    collections::HashSet,
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

#[async_trait::async_trait]
pub trait SockBackend: Send + Sync {
    async fn list(&self) -> io::Result<HashSet<events::SockKey>>;
}

pub struct SockTrayConfig {
    pulse: Duration,
}

impl Default for SockTrayConfig {
    fn default() -> Self {
        Self { pulse: Duration::from_secs(1) }
    }
}

impl SockTrayConfig {
    pub fn pulse(mut self, d: Duration) -> Self {
        self.pulse = d;
        self
    }
}

pub struct SockTray {
    cfg: SockTrayConfig,
    backend: Arc<dyn SockBackend>,
    last: HashSet<events::SockKey>,
    primed: bool,
    watch: Vec<Pattern>,
    ignore: Vec<Pattern>,
}

impl Default for SockTray {
    fn default() -> Self {
        Self::new(None)
    }
}

impl SockTray {
    pub fn new(cfg: Option<SockTrayConfig>) -> Self {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let backend: Arc<dyn SockBackend> = Arc::new(backends::linux_proc::LinuxProcBackend);

        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let backend: Arc<dyn SockBackend> = Arc::new(backends::netstat_cmd::NetstatBackend);

        Self {
            cfg: cfg.unwrap_or_default(),
            backend,
            last: HashSet::new(),
            primed: false,
            watch: Vec::new(),
            ignore: Vec::new(),
        }
    }

    pub fn set_backend<B>(&mut self, backend: B)
    where
        B: SockBackend + 'static,
    {
        self.backend = Arc::new(backend);
    }

    pub fn add(&mut self, pat: &str) {
        if let Ok(p) = Pattern::new(pat) {
            self.watch.push(p);
        }
    }

    pub fn ignore(&mut self, pat: &str) {
        if let Ok(p) = Pattern::new(pat) {
            self.ignore.push(p);
        }
    }

    async fn fire(hub: &CallbackHub<SockTrayEvent>, ev: SockTrayEvent) {
        hub.fire(ev.mask().bits(), &ev).await;
    }

    fn matches(&self, s: &events::SockKey) -> bool {
        let local = s.local_dec.as_deref().unwrap_or(&s.local);
        let remote = s.remote_dec.as_deref().unwrap_or(&s.remote);
        let state = s.state_dec.as_deref().or(s.state.as_deref()).unwrap_or("-");
        let target = format!("{} {} {} {}", s.proto, local, remote, state);

        if self.ignore.iter().any(|p| p.matches(&target)) {
            return false;
        }
        if !self.watch.is_empty() && !self.watch.iter().any(|p| p.matches(&target)) {
            return false;
        }
        true
    }

    pub async fn run(mut self, ctx: SensorCtx<SockTrayEvent>) {
        let mut ticker = tokio::time::interval(self.cfg.pulse);

        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                _ = ticker.tick() => {}
            }

            let now = match self.backend.list().await {
                Ok(v) => v,
                Err(e) => {
                    log::error!("socktray: backend list failed: {e}");
                    continue;
                }
            };

            if !self.primed {
                self.last = now;
                self.primed = true;
                continue;
            }

            let opened: Vec<_> = now.difference(&self.last).cloned().collect();
            let closed: Vec<_> = self.last.difference(&now).cloned().collect();

            for s in opened {
                if self.matches(&s) {
                    Self::fire(&ctx.hub, SockTrayEvent::Opened { sock: s }).await;
                }
            }
            for s in closed {
                if self.matches(&s) {
                    Self::fire(&ctx.hub, SockTrayEvent::Closed { sock: s }).await;
                }
            }

            self.last = now;
        }
    }
}

impl Sensor for SockTray {
    type Event = SockTrayEvent;

    fn run(self, ctx: SensorCtx<Self::Event>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { SockTray::run(self, ctx).await })
    }
}
