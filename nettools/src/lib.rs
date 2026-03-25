pub mod events;

#[cfg(test)]
mod nettools_ut;

use crate::events::NetToolsEvent;
use async_trait::async_trait;
use omnitrace_core::{
    callbacks::CallbackHub,
    sensor::{Sensor, SensorCtx},
};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    io,
    net::ToSocketAddrs,
    pin::Pin,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::TcpStream, time::timeout};

pub trait HostnameBackend: Send + Sync {
    fn current(&self) -> io::Result<String>;
}

pub trait RouteBackend: Send + Sync {
    fn list(&self) -> io::Result<Vec<events::RouteEntry>>;
}

#[async_trait]
pub trait NetHealthBackend: Send + Sync {
    async fn probe(&self, target: &events::NetHealthTarget, probe_timeout: Duration) -> io::Result<Duration>;
}

pub struct LiveHostnameBackend;
pub struct LiveRouteBackend;
pub struct LiveNetHealthBackend;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct RouteKey {
    family: events::RouteFamily,
    destination: String,
}

#[derive(Clone, Debug)]
struct NetHealthSample {
    total_probes: usize,
    successful_probes: usize,
    latency_sum_ms: u64,
}

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

impl LiveRouteBackend {
    fn current_family(line: &str, current_family: &events::RouteFamily) -> events::RouteFamily {
        if line.starts_with("Internet6:") || line.starts_with("Kernel IPv6 routing table") {
            return events::RouteFamily::Inet6;
        }

        if line.starts_with("Internet:") || line.starts_with("Kernel IP routing table") {
            return events::RouteFamily::Inet;
        }

        current_family.clone()
    }

    fn parse_line(line: &str, current_family: &events::RouteFamily) -> Option<events::RouteEntry> {
        if line.is_empty()
            || line.starts_with("Destination")
            || line.starts_with("Routing")
            || line.starts_with("Kernel")
            || line.starts_with("Internet")
        {
            return None;
        }

        let fields = line.split_whitespace().collect::<Vec<_>>();

        if fields.len() < 3 {
            return None;
        }

        Some(events::RouteEntry {
            family: if matches!(current_family, events::RouteFamily::Unknown) {
                if fields.first().is_some_and(|value| value.contains(':')) || fields.get(1).is_some_and(|value| value.contains(':')) {
                    events::RouteFamily::Inet6
                } else {
                    events::RouteFamily::Inet
                }
            } else {
                current_family.clone()
            },
            destination: fields.first().unwrap_or(&"").to_string(),
            gateway: fields.get(1).unwrap_or(&"").to_string(),
            iface: fields.last().unwrap_or(&"").to_string(),
        })
    }

    fn parse_routes(output: &str) -> Vec<events::RouteEntry> {
        output
            .lines()
            .fold((Vec::new(), events::RouteFamily::Unknown), |(mut routes, current_family), line| {
                let updated_family = Self::current_family(line.trim(), &current_family);

                if let Some(route) = Self::parse_line(line.trim(), &updated_family) {
                    routes.push(route);
                }

                (routes, updated_family)
            })
            .0
    }
}

impl RouteBackend for LiveRouteBackend {
    fn list(&self) -> io::Result<Vec<events::RouteEntry>> {
        Command::new("netstat")
            .arg("-rn")
            .output()
            .map_err(io::Error::other)
            .and_then(|output| {
                if output.status.success() {
                    Ok(Self::parse_routes(&String::from_utf8_lossy(&output.stdout)))
                } else {
                    Err(io::Error::other(String::from_utf8_lossy(&output.stderr).to_string()))
                }
            })
    }
}

#[async_trait]
impl NetHealthBackend for LiveNetHealthBackend {
    async fn probe(&self, target: &events::NetHealthTarget, probe_timeout: Duration) -> io::Result<Duration> {
        let start_time = Instant::now();
        let socket_address = format!("{}:{}", target.host, target.port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "no socket address resolved"))?;

        timeout(probe_timeout, TcpStream::connect(socket_address))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "probe timeout"))?
            .map(|_| start_time.elapsed())
    }
}

pub struct NetToolsConfig {
    pulse: Duration,
    hostname: bool,
    routes: bool,
    default_routes: bool,
    nethealth: bool,
    nethealth_window: usize,
    nethealth_timeout: Duration,
    nethealth_latency_degraded_ms: u64,
    nethealth_loss_degraded_pct: u8,
}

impl Default for NetToolsConfig {
    fn default() -> Self {
        Self {
            pulse: Duration::from_secs(3),
            hostname: true,
            routes: true,
            default_routes: true,
            nethealth: false,
            nethealth_window: 4,
            nethealth_timeout: Duration::from_secs(2),
            nethealth_latency_degraded_ms: 400,
            nethealth_loss_degraded_pct: 25,
        }
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

    pub fn hostname(mut self, hostname: bool) -> Self {
        self.hostname = hostname;
        self
    }

    pub fn routes(mut self, routes: bool) -> Self {
        self.routes = routes;
        self
    }

    pub fn default_routes(mut self, default_routes: bool) -> Self {
        self.default_routes = default_routes;
        self
    }

    pub fn nethealth(mut self, nethealth: bool) -> Self {
        self.nethealth = nethealth;
        self
    }

    pub fn nethealth_window(mut self, nethealth_window: usize) -> Self {
        self.nethealth_window = nethealth_window.max(1);
        self
    }

    pub fn nethealth_timeout(mut self, nethealth_timeout: Duration) -> Self {
        self.nethealth_timeout = nethealth_timeout;
        self
    }

    pub fn nethealth_latency_degraded_ms(mut self, nethealth_latency_degraded_ms: u64) -> Self {
        self.nethealth_latency_degraded_ms = nethealth_latency_degraded_ms;
        self
    }

    pub fn nethealth_loss_degraded_pct(mut self, nethealth_loss_degraded_pct: u8) -> Self {
        self.nethealth_loss_degraded_pct = nethealth_loss_degraded_pct;
        self
    }
}

pub struct NetTools {
    cfg: NetToolsConfig,
    hostname_backend: Arc<dyn HostnameBackend>,
    route_backend: Arc<dyn RouteBackend>,
    nethealth_backend: Arc<dyn NetHealthBackend>,
    last_hostname: Option<String>,
    last_routes: HashMap<RouteKey, events::RouteEntry>,
    nethealth_targets: Vec<events::NetHealthTarget>,
    nethealth_samples: VecDeque<NetHealthSample>,
    last_nethealth: Option<events::NetHealthState>,
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
            hostname_backend: Arc::new(LiveHostnameBackend),
            route_backend: Arc::new(LiveRouteBackend),
            nethealth_backend: Arc::new(LiveNetHealthBackend),
            last_hostname: None,
            last_routes: HashMap::new(),
            nethealth_targets: Vec::new(),
            nethealth_samples: VecDeque::new(),
            last_nethealth: None,
        }
    }

    pub fn set_hostname_backend<B>(&mut self, backend: B)
    where
        B: HostnameBackend + 'static,
    {
        self.hostname_backend = Arc::new(backend);
    }

    pub fn set_route_backend<B>(&mut self, backend: B)
    where
        B: RouteBackend + 'static,
    {
        self.route_backend = Arc::new(backend);
    }

    pub fn set_nethealth_backend<B>(&mut self, backend: B)
    where
        B: NetHealthBackend + 'static,
    {
        self.nethealth_backend = Arc::new(backend);
    }

    pub fn add_nethealth_target<S>(&mut self, host: S, port: u16)
    where
        S: Into<String>,
    {
        self.nethealth_targets.push(events::NetHealthTarget {
            host: host.into(),
            port,
        });
    }

    async fn fire(hub: &CallbackHub<NetToolsEvent>, ev: NetToolsEvent) {
        hub.fire(ev.mask().bits(), &ev).await;
    }

    fn poll_hostname(&self) -> io::Result<String> {
        self.hostname_backend.current()
    }

    fn poll_routes(&self) -> io::Result<Vec<events::RouteEntry>> {
        self.route_backend.list()
    }

    fn store_hostname(&mut self, hostname: String) {
        self.last_hostname = Some(hostname);
    }

    fn route_key(route: &events::RouteEntry) -> RouteKey {
        RouteKey { family: route.family.clone(), destination: route.destination.clone() }
    }

    fn route_map(routes: Vec<events::RouteEntry>) -> HashMap<RouteKey, events::RouteEntry> {
        routes.into_iter().map(|route| (Self::route_key(&route), route)).collect()
    }

    fn is_default_route(route: &events::RouteEntry) -> bool {
        matches!(
            route.destination.as_str(),
            "default" | "0.0.0.0/0" | "0.0.0.0" | "::/0"
        )
    }

    fn default_route(routes: &HashMap<RouteKey, events::RouteEntry>) -> Option<events::RouteEntry> {
        routes.values().find(|route| Self::is_default_route(route)).cloned()
    }

    fn trim_nethealth_samples(&mut self) {
        while self.nethealth_samples.len() > self.cfg.nethealth_window {
            self.nethealth_samples.pop_front();
        }
    }

    fn nethealth_state(&self) -> Option<events::NetHealthState> {
        if self.nethealth_samples.is_empty() {
            return None;
        }

        let (total_probes, successful_probes, latency_sum_ms) = self.nethealth_samples.iter().fold(
            (0usize, 0usize, 0u64),
            |(total_probes, successful_probes, latency_sum_ms), sample| {
                (
                    total_probes + sample.total_probes,
                    successful_probes + sample.successful_probes,
                    latency_sum_ms + sample.latency_sum_ms,
                )
            },
        );

        if total_probes == 0 {
            return None;
        }

        let loss_pct = (((total_probes - successful_probes) * 100) / total_probes) as u8;
        let avg_rtt_ms = if successful_probes > 0 {
            Some(latency_sum_ms / successful_probes as u64)
        } else {
            None
        };
        let level = if successful_probes == 0 {
            events::NetHealthLevel::Down
        } else if loss_pct >= self.cfg.nethealth_loss_degraded_pct
            || avg_rtt_ms.is_some_and(|avg_rtt_ms| avg_rtt_ms >= self.cfg.nethealth_latency_degraded_ms)
        {
            events::NetHealthLevel::Degraded
        } else {
            events::NetHealthLevel::Healthy
        };

        Some(events::NetHealthState {
            level,
            avg_rtt_ms,
            loss_pct,
            successful_probes,
            total_probes,
        })
    }

    async fn nethealth_sample(&self) -> NetHealthSample {
        let mut total_probes = 0usize;
        let mut successful_probes = 0usize;
        let mut latency_sum_ms = 0u64;

        for target in &self.nethealth_targets {
            total_probes += 1;
            if let Ok(duration) = self
                .nethealth_backend
                .probe(target, self.cfg.nethealth_timeout)
                .await
            {
                successful_probes += 1;
                latency_sum_ms += duration.as_millis() as u64;
            }
        }

        NetHealthSample {
            total_probes,
            successful_probes,
            latency_sum_ms,
        }
    }

    async fn handle_nethealth_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        if self.nethealth_targets.is_empty() {
            return;
        }

        self.nethealth_samples.push_back(self.nethealth_sample().await);
        self.trim_nethealth_samples();

        if let Some(current_nethealth) = self.nethealth_state() {
            if let Some(previous_nethealth) = self.last_nethealth.as_ref()
                && previous_nethealth != &current_nethealth
            {
                Self::fire(
                    hub,
                    NetToolsEvent::NetHealthChanged {
                        old: previous_nethealth.clone(),
                        new: current_nethealth.clone(),
                    },
                )
                .await;
            }

            self.last_nethealth = Some(current_nethealth);
        }
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

    async fn handle_route_poll(&mut self, hub: &CallbackHub<NetToolsEvent>) {
        let current_routes = match self.poll_routes() {
            Ok(current_routes) => Self::route_map(current_routes),
            Err(err) => {
                log::error!("nettools: failed to read routes: {err}");
                return;
            }
        };
        let previous_default_route = self.cfg.default_routes.then(|| Self::default_route(&self.last_routes)).flatten();
        let current_default_route = self.cfg.default_routes.then(|| Self::default_route(&current_routes)).flatten();

        if self.cfg.routes {
            for (route_key, current_route) in &current_routes {
                if let Some(previous_route) = self.last_routes.get(route_key) {
                    if previous_route != current_route {
                        Self::fire(
                            hub,
                            NetToolsEvent::RouteChanged {
                                old: previous_route.clone(),
                                new: current_route.clone(),
                            },
                        )
                        .await;
                    }
                } else {
                    Self::fire(
                        hub,
                        NetToolsEvent::RouteAdded {
                            route: current_route.clone(),
                        },
                    )
                    .await;
                }
            }

            for (route_key, previous_route) in &self.last_routes {
                if !current_routes.contains_key(route_key) {
                    Self::fire(
                        hub,
                        NetToolsEvent::RouteRemoved {
                            route: previous_route.clone(),
                        },
                    )
                    .await;
                }
            }
        }

        if let Some(current_default_route) = current_default_route.as_ref() {
            if let Some(previous_default_route) = previous_default_route.as_ref() {
                if previous_default_route != current_default_route {
                    Self::fire(
                        hub,
                        NetToolsEvent::DefaultRouteChanged {
                            old: previous_default_route.clone(),
                            new: current_default_route.clone(),
                        },
                    )
                    .await;
                }
            } else {
                Self::fire(hub, NetToolsEvent::DefaultRouteAdded { route: current_default_route.clone() }).await;
            }
        } else if let Some(previous_default_route) = previous_default_route.as_ref() {
            Self::fire(hub, NetToolsEvent::DefaultRouteRemoved { route: previous_default_route.clone() }).await;
        }

        self.last_routes = current_routes;
    }

    pub async fn run(mut self, ctx: SensorCtx<NetToolsEvent>) {
        if self.cfg.hostname {
            self.handle_hostname_poll(&ctx.hub).await;
        }

        if self.cfg.routes || self.cfg.default_routes {
            self.last_routes = Self::route_map(self.poll_routes().unwrap_or_default());
        }

        if self.cfg.nethealth {
            self.last_nethealth = None;
        }

        let mut ticker = tokio::time::interval(self.cfg.get_pulse());

        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                _ = ticker.tick() => {
                    if self.cfg.hostname {
                        self.handle_hostname_poll(&ctx.hub).await;
                    }

                    if self.cfg.routes || self.cfg.default_routes {
                        self.handle_route_poll(&ctx.hub).await;
                    }

                    if self.cfg.nethealth {
                        self.handle_nethealth_poll(&ctx.hub).await;
                    }
                },
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
