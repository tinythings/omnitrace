use crate::{events, NetHealthBackend, NetTools};
use async_trait::async_trait;
use omnitrace_core::callbacks::CallbackHub;
use std::{
    io,
    net::ToSocketAddrs,
    time::{Duration, Instant},
};
use tokio::{net::TcpStream, time::timeout};

#[derive(Clone, Debug)]
pub(crate) struct NetHealthSample {
    pub(crate) total_probes: usize,
    pub(crate) successful_probes: usize,
    pub(crate) latency_sum_ms: u64,
}

pub struct LiveNetHealthBackend;

#[async_trait]
impl NetHealthBackend for LiveNetHealthBackend {
    async fn probe(&self, target: &events::NetHealthTarget, probe_timeout: Duration) -> io::Result<Duration> {
        let now = Instant::now();
        let addr = format!("{}:{}", target.host, target.port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "no socket address resolved"))?;

        timeout(probe_timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "probe timeout"))?
            .map(|_| now.elapsed())
    }
}

impl NetTools {
    pub(crate) fn trim_nethealth_samples(&mut self) {
        while self.nethealth_samples.len() > self.cfg.nethealth_window {
            self.nethealth_samples.pop_front();
        }
    }

    pub(crate) fn nethealth_state(&self) -> Option<events::NetHealthState> {
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

        let loss = (((total_probes - successful_probes) * 100) / total_probes) as u8;
        let avg = if successful_probes > 0 {
            Some(latency_sum_ms / successful_probes as u64)
        } else {
            None
        };
        let lvl = if successful_probes == 0 {
            events::NetHealthLevel::Down
        } else if loss >= self.cfg.nethealth_loss_degraded_pct
            || avg.is_some_and(|avg| avg >= self.cfg.nethealth_latency_degraded_ms)
        {
            events::NetHealthLevel::Degraded
        } else {
            events::NetHealthLevel::Healthy
        };

        Some(events::NetHealthState {
            level: lvl,
            avg_rtt_ms: avg,
            loss_pct: loss,
            successful_probes,
            total_probes,
        })
    }

    pub(crate) async fn nethealth_sample(&self) -> NetHealthSample {
        let mut total = 0usize;
        let mut ok = 0usize;
        let mut sum = 0u64;

        for t in &self.nethealth_targets {
            total += 1;
            if let Ok(d) = self.nethealth_backend.probe(t, self.cfg.nethealth_timeout).await {
                ok += 1;
                sum += d.as_millis() as u64;
            }
        }

        NetHealthSample {
            total_probes: total,
            successful_probes: ok,
            latency_sum_ms: sum,
        }
    }

    pub(crate) async fn handle_nethealth_poll(&mut self, hub: &CallbackHub<events::NetToolsEvent>) {
        if self.nethealth_targets.is_empty() {
            return;
        }

        self.nethealth_samples.push_back(self.nethealth_sample().await);
        self.trim_nethealth_samples();

        if let Some(cur) = self.nethealth_state() {
            if let Some(old) = self.last_nethealth.as_ref()
                && old != &cur
            {
                Self::fire(
                    hub,
                    events::NetToolsEvent::NetHealthChanged {
                        old: old.clone(),
                        new: cur.clone(),
                    },
                )
                .await;
            }

            self.last_nethealth = Some(cur);
        }
    }
}
