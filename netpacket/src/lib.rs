pub mod events;
pub mod netutil;

use crate::events::{ConnKey, NetNotifyEvent};
use crate::netutil::{decode_tcp_state, is_hostish, is_ipish, reverse_dns};
use glob::Pattern;
use omnitrace_core::sensor::{Sensor, SensorCtx};
use std::collections::HashMap;
use std::time::Instant;
use std::{collections::HashSet, future::Future, io, pin::Pin, time::Duration};
use tokio::time;

pub struct NetNotifyConfig {
    pulse: Duration,
    dns: bool,
    dns_ttl: Duration,
}

impl Default for NetNotifyConfig {
    fn default() -> Self {
        Self { pulse: Duration::from_secs(1), dns: false, dns_ttl: Duration::from_secs(60) }
    }
}

impl NetNotifyConfig {
    pub fn pulse(mut self, d: Duration) -> Self {
        self.pulse = d;
        self
    }
}

pub struct NetNotify {
    cfg: NetNotifyConfig,
    last: HashSet<ConnKey>,
    is_primed: bool,
    watch: Vec<Pattern>,
    ignore: Vec<Pattern>,
    dns_cache: HashMap<std::net::IpAddr, (String, Instant)>,
    watch_ip: Vec<Pattern>,
    watch_host: Vec<Pattern>,
    ignore_ip: Vec<Pattern>,
    ignore_host: Vec<Pattern>,
}

impl Default for NetNotify {
    fn default() -> Self {
        Self::new(None)
    }
}

impl NetNotify {
    pub fn new(cfg: Option<NetNotifyConfig>) -> Self {
        Self {
            cfg: cfg.unwrap_or_default(),
            last: HashSet::new(),
            is_primed: false,
            watch: Vec::new(),
            ignore: Vec::new(),
            dns_cache: HashMap::new(),
            watch_ip: Vec::new(),
            watch_host: Vec::new(),
            ignore_ip: Vec::new(),
            ignore_host: Vec::new(),
        }
    }

    async fn fire(hub: &omnitrace_core::callbacks::CallbackHub<NetNotifyEvent>, ev: NetNotifyEvent) {
        hub.fire(ev.mask().bits(), &ev).await;
    }

    #[cfg(target_os = "linux")]
    fn read_table() -> io::Result<HashSet<ConnKey>> {
        fn parse_file(proto: &str, path: &str, is_tcp: bool, out: &mut HashSet<ConnKey>) -> io::Result<()> {
            let txt = std::fs::read_to_string(path)?;
            for (i, line) in txt.lines().enumerate() {
                use crate::netutil::decode_addr;

                if i == 0 {
                    continue;
                } // header
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() < 3 {
                    continue;
                }

                let local = cols[1];
                let remote = cols[2];
                let state = if is_tcp { cols.get(3).map(|s| s.to_string()) } else { None };

                let is_v6 = proto.ends_with('6');

                let local_dec = decode_addr(local, is_v6);
                let remote_dec = decode_addr(remote, is_v6);
                let state_dec = if is_tcp { decode_tcp_state(&state) } else { None };

                out.insert(ConnKey {
                    proto: proto.to_string(),
                    local: local.to_string(),
                    remote: remote.to_string(),
                    state,
                    local_dec,
                    remote_dec,
                    state_dec,
                    local_host: None,
                    remote_host: None,
                });
            }
            Ok(())
        }

        let mut out = HashSet::new();
        let _ = parse_file("tcp", "/proc/net/tcp", true, &mut out);
        let _ = parse_file("tcp6", "/proc/net/tcp6", true, &mut out);
        let _ = parse_file("udp", "/proc/net/udp", false, &mut out);
        let _ = parse_file("udp6", "/proc/net/udp6", false, &mut out);
        Ok(out)
    }

    #[cfg(not(target_os = "linux"))]
    fn read_table() -> io::Result<HashSet<ConnKey>> {
        Ok(HashSet::new())
    }

    pub async fn run(mut self, ctx: SensorCtx<NetNotifyEvent>) {
        let mut ticker = time::interval(self.cfg.pulse);

        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                _ = ticker.tick() => {}
            }

            let now = match Self::read_table() {
                Ok(v) => v,
                Err(e) => {
                    log::error!("netnotify: read_table failed: {e}");
                    continue;
                }
            };

            if !self.is_primed {
                self.last = now;
                self.is_primed = true;
                continue;
            }

            let opened: Vec<ConnKey> = now.difference(&self.last).cloned().collect();
            let closed: Vec<ConnKey> = self.last.difference(&now).cloned().collect();

            for mut c in opened {
                self.enrich_dns(&mut c); // remote only + cached
                if self.matches(&c) {
                    Self::fire(&ctx.hub, NetNotifyEvent::Opened { conn: c }).await;
                }
            }

            for mut c in closed {
                self.enrich_dns(&mut c);
                if self.matches(&c) {
                    Self::fire(&ctx.hub, NetNotifyEvent::Closed { conn: c }).await;
                }
            }

            self.last = now;
        }
    }

    pub fn add(&mut self, pat: &str) {
        let Ok(p) = Pattern::new(pat) else {
            return;
        };

        if is_hostish(pat) {
            self.cfg.dns = true; // auto-enable rDNS
            self.watch_host.push(p);
        } else if is_ipish(pat) {
            self.watch_ip.push(p);
        } else {
            self.watch.push(p); // fallback: your old “target string” matching
        }
    }

    pub fn ignore(&mut self, pat: &str) {
        let Ok(p) = Pattern::new(pat) else {
            return;
        };

        if is_hostish(pat) {
            self.cfg.dns = true; // still needed, because ignore can require host
            self.ignore_host.push(p);
        } else if is_ipish(pat) {
            self.ignore_ip.push(p);
        } else {
            self.ignore.push(p);
        }
    }

    pub fn dns(mut self, on: bool) -> Self {
        self.cfg.dns = on;
        self
    }

    pub fn dns_ttl(mut self, d: Duration) -> Self {
        self.cfg.dns_ttl = d;
        self
    }

    fn dns_cached(&mut self, ip: std::net::IpAddr) -> Option<String> {
        use std::time::Instant;

        // skip junk
        if matches!(ip, std::net::IpAddr::V4(v4) if v4.octets() == [0,0,0,0]) {
            return None;
        }
        if matches!(ip, std::net::IpAddr::V6(v6) if v6.octets() == [0;16]) {
            return None;
        }

        let now = Instant::now();
        if let Some((name, exp)) = self.dns_cache.get(&ip)
            && *exp > now
        {
            return Some(name.clone());
        }

        let name = reverse_dns(ip)?;
        self.dns_cache.insert(ip, (name.clone(), now + self.cfg.dns_ttl));
        Some(name)
    }

    fn enrich_dns(&mut self, c: &mut ConnKey) {
        if !self.cfg.dns {
            return;
        }

        fn ip_only(dec: &Option<String>) -> Option<std::net::IpAddr> {
            let s = dec.as_deref()?;
            let (ip, _) = s.rsplit_once(':')?;
            ip.parse().ok()
        }

        if let Some(ip) = ip_only(&c.remote_dec) {
            c.remote_host = self.dns_cached(ip);
        }
    }

    fn matches(&self, c: &ConnKey) -> bool {
        // ----- decode/normalize -----
        let local = c.local_dec.as_deref().unwrap_or(&c.local);
        let remote = c.remote_dec.as_deref().unwrap_or(&c.remote);

        // normalize proto so "udp * *" matches udp6 too
        let proto = c.proto.strip_suffix('6').unwrap_or(&c.proto);

        // DSL-friendly target: "<proto> <local> <remote>"
        let simple = format!("{} {} {}", proto, local, remote);

        // Precompute remote ip/host for the typed matchers
        let remote_dec = c.remote_dec.as_deref().unwrap_or("-");
        let remote_ip = remote_dec.rsplit_once(':').map(|(ip, _)| ip).unwrap_or(remote_dec);

        let remote_host = c.remote_host.as_deref().unwrap_or("");

        // generic ignore (DSL: "udp * *", "tcp * 1.2.3.4:*", etc)
        if self.ignore.iter().any(|p| p.matches(&simple)) {
            return false;
        }

        if !remote_host.is_empty() && self.ignore_host.iter().any(|p| p.matches(remote_host)) {
            return false;
        }
        if self.ignore_ip.iter().any(|p| p.matches(remote_ip)) {
            return false;
        }

        if !self.watch.is_empty() && !self.watch.iter().any(|p| p.matches(&simple)) {
            return false;
        }

        // Host watch: if configured, require DNS and require a host match
        if !self.watch_host.is_empty() {
            if remote_host.is_empty() {
                return false;
            }
            if !self.watch_host.iter().any(|p| p.matches(remote_host)) {
                return false;
            }
        }

        // IP watch: if configured, require match
        if !self.watch_ip.is_empty() && !self.watch_ip.iter().any(|p| p.matches(remote_ip)) {
            return false;
        }

        if !self.watch.is_empty() || !self.ignore.is_empty() {
            let target = format!(
                "{} raw:{}->{} dec:{}->{} state:{}:{}",
                proto,
                c.local,
                c.remote,
                c.local_dec.as_deref().unwrap_or("-"),
                c.remote_dec.as_deref().unwrap_or("-"),
                c.state.as_deref().unwrap_or("-"),
                c.state_dec.as_deref().unwrap_or("-"),
            );

            if !self.watch.is_empty() && !self.watch.iter().any(|p| p.matches(&target)) {
                return false;
            }

            if self.ignore.iter().any(|p| p.matches(&target)) {
                return false;
            }
        }

        true
    }
}

impl Sensor for NetNotify {
    type Event = NetNotifyEvent;

    fn run(self, ctx: SensorCtx<Self::Event>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { NetNotify::run(self, ctx).await })
    }
}
