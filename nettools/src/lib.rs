pub mod events;

#[cfg(test)]
mod nettools_ut;

use crate::events::NetToolsEvent;
use omnitrace_core::{
    callbacks::CallbackHub,
    sensor::{Sensor, SensorCtx},
};
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

pub trait HostnameBackend: Send + Sync {
    fn current(&self) -> io::Result<String>;
}

pub struct LiveHostnameBackend;

impl LiveHostnameBackend {
    fn read_hostname() -> io::Result<String> {
        let mut buf = [0u8; 256];
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast::<libc::c_char>(), buf.len()) };

        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(String::from_utf8_lossy(&buf[..buf.iter().position(|b| *b == 0).unwrap_or(buf.len())]).trim().to_string())
    }
}

impl HostnameBackend for LiveHostnameBackend {
    fn current(&self) -> io::Result<String> {
        Self::read_hostname()
    }
}

pub struct NetToolsConfig {
    pulse: Duration,
}

impl Default for NetToolsConfig {
    fn default() -> Self {
        Self { pulse: Duration::from_secs(3) }
    }
}

impl NetToolsConfig {
    pub fn pulse(mut self, pulse: Duration) -> Self {
        self.pulse = pulse;
        self
    }

    fn get_pulse(&self) -> Duration {
        self.pulse
    }
}

pub struct NetTools {
    cfg: NetToolsConfig,
    backend: Arc<dyn HostnameBackend>,
    last_hostname: Option<String>,
}

impl Default for NetTools {
    fn default() -> Self {
        Self::new(None)
    }
}

impl NetTools {
    pub fn new(cfg: Option<NetToolsConfig>) -> Self {
        Self {
            cfg: cfg.unwrap_or_default(),
            backend: Arc::new(LiveHostnameBackend),
            last_hostname: None,
        }
    }

    pub fn set_backend<B>(&mut self, backend: B)
    where
        B: HostnameBackend + 'static,
    {
        self.backend = Arc::new(backend);
    }

    async fn fire(hub: &CallbackHub<NetToolsEvent>, ev: NetToolsEvent) {
        hub.fire(ev.mask().bits(), &ev).await;
    }

    fn poll_hostname(&self) -> io::Result<String> {
        self.backend.current()
    }

    fn store_hostname(&mut self, hostname: String) {
        self.last_hostname = Some(hostname);
    }

    async fn handle_hostname_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        let current_hostname = match self.poll_hostname() {
            Ok(current_hostname) => current_hostname,
            Err(err) => {
                log::error!("nettools: failed to read hostname: {err}");
                return;
            }
        };

        if let Some(previous_hostname) = self.last_hostname.as_ref()
            && previous_hostname != &current_hostname
        {
            Self::fire(
                hub,
                NetToolsEvent::HostnameChanged { old: previous_hostname.clone(), new: current_hostname.clone() },
            )
            .await;
        }

        self.store_hostname(current_hostname);
    }

    pub async fn run(mut self, ctx: SensorCtx<NetToolsEvent>) {
        self.handle_hostname_poll(&ctx.hub).await;

        let mut ticker = tokio::time::interval(self.cfg.get_pulse());

        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                _ = ticker.tick() => self.handle_hostname_poll(&ctx.hub).await,
            }
        }
    }
}

impl Sensor for NetTools {
    type Event = NetToolsEvent;

    fn run(self, ctx: SensorCtx<Self::Event>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { self.run(ctx).await })
    }
}
