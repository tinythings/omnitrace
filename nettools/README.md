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

This crate is intended to hold things such as:

- hostname change detection
- routing table observation
- default route changes
- socket and listener inspection
- small low-level network helpers for minimal systems

The goal is to keep these host-network identity features together, instead of
mixing them into unrelated sensors.

Example binaries:

- `cargo run -p nettools --bin nettools-hostchange`
- `cargo run -p nettools --bin nettools-routes`
- `cargo run -p nettools --bin nettools-default-route`
- `cargo run -p nettools --bin nettools-nethealth`
- `cargo run -p nettools --bin nettools-sockets`
