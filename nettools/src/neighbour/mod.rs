use crate::{events, NeighbourBackend, NetTools};
use omnitrace_core::callbacks::CallbackHub;
use std::{collections::HashMap, io, process::Command};

pub struct LiveNeighbourBackend;

impl LiveNeighbourBackend {
    pub(crate) fn parse_proc_net_arp(content: &str) -> HashMap<String, events::NeighbourEntry> {
        content
            .lines()
            .enumerate()
            .skip(1)
            .filter_map(|(_, line)| {
                let cs = line.split_whitespace().collect::<Vec<_>>();
                (cs.len() >= 6).then(|| {
                    (
                        cs[0].to_string(),
                        events::NeighbourEntry {
                            address: cs[0].to_string(),
                            mac: cs[3].to_string(),
                            iface: cs[5].to_string(),
                            state: Some(cs[2].to_string()),
                        },
                    )
                })
            })
            .collect()
    }

    pub(crate) fn parse_arp_line(line: &str) -> Option<events::NeighbourEntry> {
        let a = line
            .split_whitespace()
            .find(|field| field.parse::<std::net::IpAddr>().is_ok())
            .map(str::to_string)?;
        let m = line
            .split_whitespace()
            .find(|field| field.chars().filter(|ch| *ch == ':').count() == 5)
            .map(str::to_string)?;
        let i = line
            .split_whitespace()
            .rev()
            .find(|field| field.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.'))
            .map(str::to_string)
            .unwrap_or_default();

        Some(events::NeighbourEntry {
            address: a.clone(),
            mac: m,
            iface: i,
            state: None,
        })
    }

    pub(crate) fn parse_ip_neigh_line(line: &str) -> Option<events::NeighbourEntry> {
        let fs = line.split_whitespace().collect::<Vec<_>>();
        let a = fs
            .first()
            .filter(|field| field.parse::<std::net::IpAddr>().is_ok())
            .map(|field| (*field).to_string())?;
        let i = fs
            .windows(2)
            .find(|window| window[0] == "dev")
            .map(|window| window[1].to_string())
            .unwrap_or_default();
        let m = fs
            .windows(2)
            .find(|window| matches!(window[0], "lladdr" | "at"))
            .map(|window| window[1].to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let s = fs
            .iter()
            .rev()
            .find(|field| field.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_'))
            .map(|field| (*field).to_string());

        Some(events::NeighbourEntry {
            address: a.clone(),
            mac: m,
            iface: i,
            state: s,
        })
    }

    fn parse_neighbour_table(content: &str) -> HashMap<String, events::NeighbourEntry> {
        content
            .lines()
            .filter_map(Self::parse_ip_neigh_line)
            .map(|neighbour| (neighbour.address.clone(), neighbour))
            .collect()
    }
}

impl NeighbourBackend for LiveNeighbourBackend {
    fn list(&self) -> io::Result<HashMap<String, events::NeighbourEntry>> {
        std::fs::read_to_string("/proc/net/arp")
            .map(|content| Self::parse_proc_net_arp(&content))
            .or_else(|_| std::fs::read_to_string("/proc/net/ndisc_cache").map(|content| Self::parse_neighbour_table(&content)))
            .or_else(|_| {
                Command::new("ip")
                    .arg("neigh")
                    .output()
                    .map_err(io::Error::other)
                    .and_then(|output| {
                        if output.status.success() {
                            Ok(Self::parse_neighbour_table(&String::from_utf8_lossy(&output.stdout)))
                        } else {
                            Err(io::Error::other(String::from_utf8_lossy(&output.stderr).to_string()))
                        }
                    })
            })
            .or_else(|_| {
                Command::new("arp")
                    .arg("-an")
                    .output()
                    .map_err(io::Error::other)
                    .and_then(|output| {
                        if output.status.success() {
                            Ok(String::from_utf8_lossy(&output.stdout)
                                .lines()
                                .filter_map(Self::parse_arp_line)
                                .map(|neighbour| (neighbour.address.clone(), neighbour))
                                .collect())
                        } else {
                            Err(io::Error::other(String::from_utf8_lossy(&output.stderr).to_string()))
                        }
                    })
            })
    }
}

impl NetTools {
    pub(crate) fn poll_neighbours(&self) -> io::Result<HashMap<String, events::NeighbourEntry>> {
        self.neighbour_backend.list()
    }

    pub(crate) async fn handle_neighbour_poll(&mut self, hub: &CallbackHub<events::NetToolsEvent>) {
        let cur = match self.poll_neighbours() {
            Ok(cur) => cur,
            Err(err) => {
                log::error!("nettools: failed to read neighbours: {err}");
                return;
            }
        };

        for (a, cur_n) in &cur {
            if let Some(old) = self.last_neighbours.get(a) {
                if old != cur_n {
                    Self::fire(
                        hub,
                        events::NetToolsEvent::NeighbourChanged {
                            old: old.clone(),
                            new: cur_n.clone(),
                        },
                    )
                    .await;
                }
            } else {
                Self::fire(
                    hub,
                    events::NetToolsEvent::NeighbourAdded {
                        neighbour: cur_n.clone(),
                    },
                )
                .await;
            }
        }

        for (a, old) in &self.last_neighbours {
            if !cur.contains_key(a) {
                Self::fire(
                    hub,
                    events::NetToolsEvent::NeighbourRemoved {
                        neighbour: old.clone(),
                    },
                )
                .await;
            }
        }

        self.last_neighbours = cur;
    }
}
