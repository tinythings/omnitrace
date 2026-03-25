use crate::{events, NetTools, ThroughputBackend};
use omnitrace_core::callbacks::CallbackHub;
use std::{
    collections::HashMap,
    io,
    process::Command,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub(crate) struct ThroughputState {
    pub(crate) at: Instant,
    pub(crate) counters: HashMap<String, events::InterfaceCounters>,
}

pub struct LiveThroughputBackend;

impl LiveThroughputBackend {
    pub(crate) fn parse_proc_net_dev_line(line: &str) -> Option<events::InterfaceCounters> {
        line.split_once(':').and_then(|(iface, stats)| {
            stats
                .split_whitespace()
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()
                .filter(|values| values.len() >= 16)
                .map(|values| events::InterfaceCounters {
                    iface: iface.trim().to_string(),
                    rx_bytes: values[0],
                    rx_packets: values[1],
                    rx_errors: values[2],
                    rx_drops: values[3],
                    tx_bytes: values[8],
                    tx_packets: values[9],
                    tx_errors: values[10],
                    tx_drops: values[11],
                })
        })
    }

    fn parse_proc_net_dev(content: &str) -> HashMap<String, events::InterfaceCounters> {
        content
            .lines()
            .skip(2)
            .filter_map(Self::parse_proc_net_dev_line)
            .map(|counters| (counters.iface.clone(), counters))
            .collect()
    }

    pub(crate) fn parse_netstat_ib_line(line: &str) -> Option<events::InterfaceCounters> {
        let fs = line.split_whitespace().collect::<Vec<_>>();
        (fs.len() >= 10)
            .then(|| events::InterfaceCounters {
                iface: fs[0].to_string(),
                rx_packets: fs.iter().rev().nth(3).and_then(|v| v.parse().ok()).unwrap_or(0),
                rx_errors: 0,
                rx_drops: 0,
                tx_packets: fs.iter().rev().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0),
                tx_errors: 0,
                tx_drops: 0,
                rx_bytes: fs.iter().rev().nth(2).and_then(|v| v.parse().ok()).unwrap_or(0),
                tx_bytes: fs.last().and_then(|v| v.parse().ok()).unwrap_or(0),
            })
            .filter(|counters| !matches!(counters.iface.as_str(), "Name" | "Iface"))
    }

    fn parse_netstat_ib(content: &str) -> HashMap<String, events::InterfaceCounters> {
        content
            .lines()
            .filter_map(Self::parse_netstat_ib_line)
            .map(|counters| (counters.iface.clone(), counters))
            .collect()
    }
}

impl ThroughputBackend for LiveThroughputBackend {
    fn list(&self) -> io::Result<HashMap<String, events::InterfaceCounters>> {
        std::fs::read_to_string("/proc/net/dev")
            .map(|content| Self::parse_proc_net_dev(&content))
            .or_else(|_| {
                Command::new("netstat")
                    .args(["-ibn"])
                    .output()
                    .map_err(io::Error::other)
                    .and_then(|output| {
                        if output.status.success() {
                            Ok(Self::parse_netstat_ib(&String::from_utf8_lossy(&output.stdout)))
                        } else {
                            Err(io::Error::other(String::from_utf8_lossy(&output.stderr).to_string()))
                        }
                    })
            })
    }
}

impl NetTools {
    pub(crate) fn poll_throughput(&self) -> io::Result<HashMap<String, events::InterfaceCounters>> {
        self.throughput_backend.list()
    }

    pub(crate) fn throughput_sample(
        previous: &events::InterfaceCounters,
        current: &events::InterfaceCounters,
        interval: Duration,
    ) -> Option<events::ThroughputSample> {
        (interval.as_millis() > 0).then(|| events::ThroughputSample {
            iface: current.iface.clone(),
            interval_ms: interval.as_millis() as u64,
            rx_bytes_per_sec: current.rx_bytes.saturating_sub(previous.rx_bytes).saturating_mul(1000)
                / interval.as_millis() as u64,
            tx_bytes_per_sec: current.tx_bytes.saturating_sub(previous.tx_bytes).saturating_mul(1000)
                / interval.as_millis() as u64,
            rx_packets_per_sec: current.rx_packets.saturating_sub(previous.rx_packets).saturating_mul(1000)
                / interval.as_millis() as u64,
            tx_packets_per_sec: current.tx_packets.saturating_sub(previous.tx_packets).saturating_mul(1000)
                / interval.as_millis() as u64,
            counters: current.clone(),
        })
    }

    pub(crate) async fn handle_throughput_poll(&mut self, hub: &CallbackHub<events::NetToolsEvent>) {
        let cur = match self.poll_throughput() {
            Ok(counters) => ThroughputState {
                at: Instant::now(),
                counters,
            },
            Err(err) => {
                log::error!("nettools: failed to read interface counters: {err}");
                return;
            }
        };

        if let Some(old_s) = self.last_throughput.as_ref() {
            for s in cur
                .counters
                .iter()
                .filter_map(|(i, cur_c)| {
                    old_s.counters.get(i).and_then(|old_c| {
                        Self::throughput_sample(old_c, cur_c, cur.at.saturating_duration_since(old_s.at))
                    })
                })
                .filter(|s| {
                    s.rx_bytes_per_sec > 0
                        || s.tx_bytes_per_sec > 0
                        || s.rx_packets_per_sec > 0
                        || s.tx_packets_per_sec > 0
                })
            {
                Self::fire(hub, events::NetToolsEvent::ThroughputUpdated { sample: s }).await;
            }
        }

        self.last_throughput = Some(cur);
    }
}
