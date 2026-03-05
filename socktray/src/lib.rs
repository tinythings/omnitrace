pub mod backends;
pub mod events;

use crate::events::SockTrayEvent;
use glob::Pattern;
use omnitrace_core::{
    callbacks::CallbackHub,
    sensor::{Sensor, SensorCtx},
};
use std::{
    collections::{HashMap, HashSet},
    ffi::CStr,
    future::Future,
    io,
    net::IpAddr,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

#[async_trait::async_trait]
pub trait SockBackend: Send + Sync {
    async fn list(&self) -> io::Result<HashSet<events::SockKey>>;
}

pub struct SockTrayConfig {
    pulse: Duration,
    dns: bool,
    dns_ttl: Duration,
}

impl Default for SockTrayConfig {
    fn default() -> Self {
        Self { pulse: Duration::from_secs(1), dns: false, dns_ttl: Duration::from_secs(60) }
    }
}

impl SockTrayConfig {
    pub fn pulse(mut self, d: Duration) -> Self {
        self.pulse = d;
        self
    }

    pub fn dns(mut self, on: bool) -> Self {
        self.dns = on;
        self
    }

    pub fn dns_ttl(mut self, d: Duration) -> Self {
        self.dns_ttl = d;
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
    dns_cache: HashMap<IpAddr, (String, Instant)>,
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
            dns_cache: HashMap::new(),
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
        let host = s.remote_host.as_deref().unwrap_or("-");
        let state = s.state_dec.as_deref().or(s.state.as_deref()).unwrap_or("-");
        let target = format!("{} {} {} {} {}", s.proto, local, remote, host, state);

        if self.ignore.iter().any(|p| p.matches(&target)) {
            return false;
        }
        if !self.watch.is_empty() && !self.watch.iter().any(|p| p.matches(&target)) {
            return false;
        }
        true
    }

    fn reverse_dns(ip: IpAddr) -> Option<String> {
        unsafe {
            let mut host = [0i8; 1025];

            let rc = match ip {
                IpAddr::V4(v4) => {
                    let mut sa: libc::sockaddr_in = std::mem::zeroed();
                    sa.sin_family = libc::AF_INET as _;
                    sa.sin_port = 0;
                    sa.sin_addr = libc::in_addr { s_addr: u32::from_be_bytes(v4.octets()) };

                    libc::getnameinfo(
                        (&sa as *const libc::sockaddr_in).cast::<libc::sockaddr>(),
                        std::mem::size_of::<libc::sockaddr_in>() as _,
                        host.as_mut_ptr(),
                        host.len() as _,
                        std::ptr::null_mut(),
                        0,
                        libc::NI_NAMEREQD,
                    )
                }
                IpAddr::V6(v6) => {
                    let mut sa: libc::sockaddr_in6 = std::mem::zeroed();
                    sa.sin6_family = libc::AF_INET6 as _;
                    sa.sin6_port = 0;
                    sa.sin6_addr = libc::in6_addr { s6_addr: v6.octets() };

                    libc::getnameinfo(
                        (&sa as *const libc::sockaddr_in6).cast::<libc::sockaddr>(),
                        std::mem::size_of::<libc::sockaddr_in6>() as _,
                        host.as_mut_ptr(),
                        host.len() as _,
                        std::ptr::null_mut(),
                        0,
                        libc::NI_NAMEREQD,
                    )
                }
            };

            if rc != 0 {
                return None;
            }
            CStr::from_ptr(host.as_ptr()).to_str().ok().map(|s| s.to_string())
        }
    }

    fn parse_remote_ip(s: &events::SockKey) -> Option<IpAddr> {
        let dec = s.remote_dec.as_deref().unwrap_or(&s.remote);
        let (ip, _) = dec.rsplit_once(':')?;
        ip.parse().ok()
    }

    fn dns_cached(&mut self, ip: IpAddr) -> Option<String> {
        let now = Instant::now();
        if let Some((name, exp)) = self.dns_cache.get(&ip)
            && *exp > now
        {
            return Some(name.clone());
        }

        let name = Self::reverse_dns(ip)?;
        self.dns_cache.insert(ip, (name.clone(), now + self.cfg.dns_ttl));
        Some(name)
    }

    fn enrich_dns(&mut self, s: &mut events::SockKey) {
        if !self.cfg.dns {
            return;
        }
        if s.remote_host.is_some() {
            return;
        }
        let Some(ip) = Self::parse_remote_ip(s) else {
            return;
        };
        s.remote_host = self.dns_cached(ip);
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

            for mut s in opened {
                self.enrich_dns(&mut s);
                if self.matches(&s) {
                    Self::fire(&ctx.hub, SockTrayEvent::Opened { sock: s }).await;
                }
            }
            for mut s in closed {
                self.enrich_dns(&mut s);
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
