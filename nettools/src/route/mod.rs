use crate::{events, NetTools, RouteBackend};
use omnitrace_core::callbacks::CallbackHub;
use std::{collections::HashMap, io, process::Command};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct RouteKey {
    pub(crate) family: events::RouteFamily,
    pub(crate) destination: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct RouteLookupKey {
    pub(crate) target: String,
}

pub struct LiveRouteBackend;

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

    pub(crate) fn parse_line(line: &str, current_family: &events::RouteFamily) -> Option<events::RouteEntry> {
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

    pub(crate) fn parse_routes(output: &str) -> Vec<events::RouteEntry> {
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

impl NetTools {
    pub(crate) fn poll_routes(&self) -> io::Result<Vec<events::RouteEntry>> {
        self.route_backend.list()
    }

    pub(crate) fn route_key(route: &events::RouteEntry) -> RouteKey {
        RouteKey {
            family: route.family.clone(),
            destination: route.destination.clone(),
        }
    }

    pub(crate) fn route_map(routes: Vec<events::RouteEntry>) -> HashMap<RouteKey, events::RouteEntry> {
        routes.into_iter().map(|route| (Self::route_key(&route), route)).collect()
    }

    pub(crate) fn route_lookup_key(lookup: &events::RouteLookupEntry) -> RouteLookupKey {
        RouteLookupKey {
            target: lookup.target.clone(),
        }
    }

    pub(crate) fn is_default_route(route: &events::RouteEntry) -> bool {
        matches!(route.destination.as_str(), "default" | "0.0.0.0/0" | "0.0.0.0" | "::/0")
    }

    pub(crate) fn default_route(routes: &HashMap<RouteKey, events::RouteEntry>) -> Option<events::RouteEntry> {
        routes.values().find(|route| Self::is_default_route(route)).cloned()
    }

    pub(crate) fn parse_target(value: &str) -> Option<std::net::IpAddr> {
        value.parse::<std::net::IpAddr>().ok()
    }

    pub(crate) fn route_prefix_len(route: &events::RouteEntry) -> Option<u8> {
        match route.destination.as_str() {
            "default" | "0.0.0.0" | "0.0.0.0/0" | "::/0" => Some(0),
            destination => destination
                .split_once('/')
                .and_then(|(_, prefix)| prefix.parse::<u8>().ok())
                .or_else(|| Self::parse_target(destination).map(|ip| if ip.is_ipv4() { 32 } else { 128 })),
        }
    }

    pub(crate) fn route_matches_target(route: &events::RouteEntry, target: std::net::IpAddr) -> bool {
        match (target, route.destination.as_str()) {
            (std::net::IpAddr::V4(_), "default" | "0.0.0.0" | "0.0.0.0/0") => true,
            (std::net::IpAddr::V6(_), "::/0") => true,
            (std::net::IpAddr::V4(target), destination) => destination
                .split_once('/')
                .and_then(|(network, prefix)| {
                    let network = network.parse::<std::net::Ipv4Addr>().ok()?;
                    let prefix = prefix.parse::<u32>().ok()?;
                    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
                    Some((u32::from(target) & mask) == (u32::from(network) & mask))
                })
                .or_else(|| destination.parse::<std::net::Ipv4Addr>().ok().map(|network| network == target))
                .unwrap_or(false),
            (std::net::IpAddr::V6(target), destination) => destination
                .split_once('/')
                .and_then(|(network, prefix)| {
                    let network = network.parse::<std::net::Ipv6Addr>().ok()?;
                    let prefix = prefix.parse::<u32>().ok()?;
                    let target = u128::from(target);
                    let network = u128::from(network);
                    let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
                    Some((target & mask) == (network & mask))
                })
                .or_else(|| destination.parse::<std::net::Ipv6Addr>().ok().map(|network| network == target))
                .unwrap_or(false),
        }
    }

    pub(crate) fn route_lookup(
        routes: &HashMap<RouteKey, events::RouteEntry>,
        target: &str,
    ) -> Option<events::RouteLookupEntry> {
        let ip = Self::parse_target(target)?;
        routes
            .values()
            .filter(|route| {
                matches!(
                    (&ip, &route.family),
                    (std::net::IpAddr::V4(_), events::RouteFamily::Inet)
                        | (std::net::IpAddr::V6(_), events::RouteFamily::Inet6)
                        | (_, events::RouteFamily::Unknown)
                ) && Self::route_matches_target(route, ip)
            })
            .max_by_key(|route| Self::route_prefix_len(route).unwrap_or(0))
            .cloned()
            .map(|route| events::RouteLookupEntry {
                target: target.to_string(),
                route,
            })
    }

    pub(crate) fn route_lookup_map(
        routes: &HashMap<RouteKey, events::RouteEntry>,
        targets: &[String],
    ) -> HashMap<RouteLookupKey, events::RouteLookupEntry> {
        targets
            .iter()
            .filter_map(|target| Self::route_lookup(routes, target))
            .map(|lookup| (Self::route_lookup_key(&lookup), lookup))
            .collect()
    }

    pub(crate) async fn handle_route_poll(&mut self, hub: &CallbackHub<events::NetToolsEvent>) {
        let cur = match self.poll_routes() {
            Ok(cur) => Self::route_map(cur),
            Err(err) => {
                log::error!("nettools: failed to read routes: {err}");
                return;
            }
        };
        let old_def = self.cfg.default_routes.then(|| Self::default_route(&self.last_routes)).flatten();
        let cur_def = self.cfg.default_routes.then(|| Self::default_route(&cur)).flatten();
        let cur_lu = if self.cfg.route_lookups {
            Self::route_lookup_map(&cur, &self.route_lookup_targets)
        } else {
            Default::default()
        };

        if self.cfg.routes {
            for (k, cur_r) in &cur {
                if let Some(old) = self.last_routes.get(k) {
                    if old != cur_r {
                        Self::fire(
                            hub,
                            events::NetToolsEvent::RouteChanged {
                                old: old.clone(),
                                new: cur_r.clone(),
                            },
                        )
                        .await;
                    }
                } else {
                    Self::fire(
                        hub,
                        events::NetToolsEvent::RouteAdded {
                            route: cur_r.clone(),
                        },
                    )
                    .await;
                }
            }

            for (k, old) in &self.last_routes {
                if !cur.contains_key(k) {
                    Self::fire(
                        hub,
                        events::NetToolsEvent::RouteRemoved {
                            route: old.clone(),
                        },
                    )
                    .await;
                }
            }
        }

        if let Some(cur_r) = cur_def.as_ref() {
            if let Some(old) = old_def.as_ref() {
                if old != cur_r {
                    Self::fire(
                        hub,
                        events::NetToolsEvent::DefaultRouteChanged {
                            old: old.clone(),
                            new: cur_r.clone(),
                        },
                    )
                    .await;
                }
            } else {
                Self::fire(
                    hub,
                    events::NetToolsEvent::DefaultRouteAdded {
                        route: cur_r.clone(),
                    },
                )
                .await;
            }
        } else if let Some(old) = old_def.as_ref() {
            Self::fire(
                hub,
                events::NetToolsEvent::DefaultRouteRemoved {
                    route: old.clone(),
                },
            )
            .await;
        }

        if self.cfg.route_lookups {
            for (k, cur_r) in &cur_lu {
                if let Some(old) = self.last_route_lookups.get(k) {
                    if old != cur_r {
                        Self::fire(
                            hub,
                            events::NetToolsEvent::RouteLookupChanged {
                                old: old.clone(),
                                new: cur_r.clone(),
                            },
                        )
                        .await;
                    }
                } else {
                    Self::fire(
                        hub,
                        events::NetToolsEvent::RouteLookupAdded {
                            lookup: cur_r.clone(),
                        },
                    )
                    .await;
                }
            }

            for (k, old) in &self.last_route_lookups {
                if !cur_lu.contains_key(k) {
                    Self::fire(
                        hub,
                        events::NetToolsEvent::RouteLookupRemoved {
                            lookup: old.clone(),
                        },
                    )
                    .await;
                }
            }
        }

        self.last_routes = cur;
        self.last_route_lookups = cur_lu;
    }
}
