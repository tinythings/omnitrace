# nettools

`nettools` is a host-network and identity sensor crate for Omnitrace.

The first feature is hostname change detection.

Right now it can:

- read the live system hostname
- poll for hostname changes
- emit a `HostnameChanged` event when the hostname changes
- read the routing table
- emit route added, removed, and changed events
- detect default route changes as first-class events
- run active network health probes
- emit nethealth change events for latency spikes, loss, and outages
- inspect live sockets, listeners, and connections
- emit socket added and removed events
- inspect ARP and neighbour-table entries
- emit neighbour added, removed, and changed events
- watch route lookup results for specific destinations
- emit route lookup added, removed, and changed events
- read interface counters and compute per-interface throughput
- emit throughput update events when traffic changes
- read live Wi-Fi link quality and radio levels where the host exposes them
- emit Wi-Fi added, removed, and changed events
- supports both IPv4 and IPv6 where the host exposes that data

This crate is intended to hold things such as:

- hostname change detection
- routing table observation
- default route changes
- socket and listener inspection
- ARP and neighbour-table inspection
- route lookup for specific destinations
- interface counters and throughput
- Wi-Fi quality and radio details
- small low-level network helpers for minimal systems

The goal is to keep these host-network identity features together, instead of
mixing them into unrelated sensors.

Example binaries:

- `cargo run -p nettools --bin nettools-hostchange`
- `cargo run -p nettools --bin nettools-routes`
- `cargo run -p nettools --bin nettools-default-route`
- `cargo run -p nettools --bin nettools-nethealth`
- `cargo run -p nettools --bin nettools-sockets`
- `cargo run -p nettools --bin nettools-neighbours`
- `cargo run -p nettools --bin nettools-route-lookup -- 8.8.8.8 2001:4860:4860::8888`
- `cargo run -p nettools --bin nettools-throughput`
- `cargo run -p nettools --bin nettools-wifi`
