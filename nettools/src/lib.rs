pub mod events;
mod hostname;
mod neighbour;
mod nethealth;
mod route;
mod socket;
mod throughput;
mod wifi;

#[cfg(test)]
mod neighbour_ut;
#[cfg(test)]
mod nethealth_ut;
#[cfg(test)]
mod nettools_ut;
#[cfg(test)]
mod route_ut;
#[cfg(test)]
mod socket_ut;
#[cfg(test)]
mod throughput_ut;
#[cfg(test)]
mod wifi_ut;

use crate::events::NetToolsEvent;
use async_trait::async_trait;
use omnitrace_core::{
    callbacks::CallbackHub,
    sensor::{Sensor, SensorCtx},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

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

pub trait SocketBackend: Send + Sync {
    fn list(&self) -> io::Result<HashSet<events::SocketEntry>>;
}

pub trait NeighbourBackend: Send + Sync {
    fn list(&self) -> io::Result<HashMap<String, events::NeighbourEntry>>;
}

pub trait ThroughputBackend: Send + Sync {
    fn list(&self) -> io::Result<HashMap<String, events::InterfaceCounters>>;
}

pub trait WifiBackend: Send + Sync {
    fn list(&self) -> io::Result<HashMap<String, events::WifiDetails>>;
}

pub use hostname::LiveHostnameBackend;
pub use neighbour::LiveNeighbourBackend;
pub use nethealth::LiveNetHealthBackend;
pub use route::LiveRouteBackend;
pub use socket::LiveSocketBackend;
pub use throughput::LiveThroughputBackend;
pub use wifi::LiveWifiBackend;
use nethealth::NetHealthSample;
use route::{RouteKey, RouteLookupKey};
use throughput::ThroughputState;

pub struct NetToolsConfig {
    pulse: Duration,
    hostname: bool,
    routes: bool,
    default_routes: bool,
    nethealth: bool,
    sockets: bool,
    neighbours: bool,
    route_lookups: bool,
    throughput: bool,
    wifi: bool,
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
            sockets: false,
            neighbours: false,
            route_lookups: false,
            throughput: false,
            wifi: false,
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

    pub fn sockets(mut self, sockets: bool) -> Self {
        self.sockets = sockets;
        self
    }

    pub fn neighbours(mut self, neighbours: bool) -> Self {
        self.neighbours = neighbours;
        self
    }

    pub fn route_lookups(mut self, route_lookups: bool) -> Self {
        self.route_lookups = route_lookups;
        self
    }

    pub fn throughput(mut self, throughput: bool) -> Self {
        self.throughput = throughput;
        self
    }

    pub fn wifi(mut self, wifi: bool) -> Self {
        self.wifi = wifi;
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
    socket_backend: Arc<dyn SocketBackend>,
    neighbour_backend: Arc<dyn NeighbourBackend>,
    throughput_backend: Arc<dyn ThroughputBackend>,
    wifi_backend: Arc<dyn WifiBackend>,
    last_hostname: Option<String>,
    last_routes: HashMap<RouteKey, events::RouteEntry>,
    nethealth_targets: Vec<events::NetHealthTarget>,
    nethealth_samples: VecDeque<NetHealthSample>,
    last_nethealth: Option<events::NetHealthState>,
    last_sockets: HashSet<events::SocketEntry>,
    last_neighbours: HashMap<String, events::NeighbourEntry>,
    route_lookup_targets: Vec<String>,
    last_route_lookups: HashMap<RouteLookupKey, events::RouteLookupEntry>,
    last_throughput: Option<ThroughputState>,
    last_wifi: HashMap<String, events::WifiDetails>,
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
            socket_backend: Arc::new(LiveSocketBackend),
            neighbour_backend: Arc::new(LiveNeighbourBackend),
            throughput_backend: Arc::new(LiveThroughputBackend),
            wifi_backend: Arc::new(LiveWifiBackend),
            last_hostname: None,
            last_routes: HashMap::new(),
            nethealth_targets: Vec::new(),
            nethealth_samples: VecDeque::new(),
            last_nethealth: None,
            last_sockets: HashSet::new(),
            last_neighbours: HashMap::new(),
            route_lookup_targets: Vec::new(),
            last_route_lookups: HashMap::new(),
            last_throughput: None,
            last_wifi: HashMap::new(),
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

    pub fn set_socket_backend<B>(&mut self, backend: B)
    where
        B: SocketBackend + 'static,
    {
        self.socket_backend = Arc::new(backend);
    }

    pub fn set_neighbour_backend<B>(&mut self, backend: B)
    where
        B: NeighbourBackend + 'static,
    {
        self.neighbour_backend = Arc::new(backend);
    }

    pub fn set_throughput_backend<B>(&mut self, backend: B)
    where
        B: ThroughputBackend + 'static,
    {
        self.throughput_backend = Arc::new(backend);
    }

    pub fn set_wifi_backend<B>(&mut self, backend: B)
    where
        B: WifiBackend + 'static,
    {
        self.wifi_backend = Arc::new(backend);
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

    pub fn add_route_lookup_target<S>(&mut self, target: S)
    where
        S: Into<String>,
    {
        self.route_lookup_targets.push(target.into());
    }

    async fn fire(hub: &CallbackHub<NetToolsEvent>, ev: NetToolsEvent) {
        hub.fire(ev.mask().bits(), &ev).await;
    }

    pub async fn run(mut self, ctx: SensorCtx<NetToolsEvent>) {
        if self.cfg.hostname {
            self.handle_hostname_poll(&ctx.hub).await;
        }

        if self.cfg.routes || self.cfg.default_routes || self.cfg.route_lookups {
            self.last_routes = Self::route_map(self.poll_routes().unwrap_or_default());
            self.last_route_lookups = Self::route_lookup_map(&self.last_routes, &self.route_lookup_targets);
        }

        if self.cfg.nethealth {
            self.last_nethealth = None;
        }

        if self.cfg.sockets {
            self.last_sockets = self.poll_sockets().unwrap_or_default();
        }

        if self.cfg.neighbours {
            self.last_neighbours = self.poll_neighbours().unwrap_or_default();
        }

        if self.cfg.throughput {
            self.last_throughput = self.poll_throughput().ok().map(|counters| ThroughputState {
                at: Instant::now(),
                counters,
            });
        }

        if self.cfg.wifi {
            self.last_wifi = self.poll_wifi().unwrap_or_default();
        }

        let mut ticker = tokio::time::interval(self.cfg.get_pulse());

        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                _ = ticker.tick() => {
                    if self.cfg.hostname {
                        self.handle_hostname_poll(&ctx.hub).await;
                    }

                    if self.cfg.routes || self.cfg.default_routes || self.cfg.route_lookups {
                        self.handle_route_poll(&ctx.hub).await;
                    }

                    if self.cfg.nethealth {
                        self.handle_nethealth_poll(&ctx.hub).await;
                    }

                    if self.cfg.sockets {
                        self.handle_socket_poll(&ctx.hub).await;
                    }

                    if self.cfg.neighbours {
                        self.handle_neighbour_poll(&ctx.hub).await;
                    }

                    if self.cfg.throughput {
                        self.handle_throughput_poll(&ctx.hub).await;
                    }

                    if self.cfg.wifi {
                        self.handle_wifi_poll(&ctx.hub).await;
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
