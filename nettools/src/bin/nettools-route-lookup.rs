use async_trait::async_trait;
use nettools::events::{NetToolsEvent, NetToolsMask};
use nettools::{NetTools, NetToolsConfig};
use omnitrace_core::callbacks::{Callback, CallbackHub, CallbackResult};
use omnitrace_core::sensor::spawn_sensor;
use std::{env, sync::Arc, time::Duration};
use tokio::sync::mpsc::channel;

struct JsonCb;

#[async_trait]
impl Callback<NetToolsEvent> for JsonCb {
    fn mask(&self) -> u64 {
        (NetToolsMask::ROUTE_LOOKUP_ADDED | NetToolsMask::ROUTE_LOOKUP_REMOVED | NetToolsMask::ROUTE_LOOKUP_CHANGED).bits()
    }

    async fn call(&self, ev: &NetToolsEvent) -> Option<CallbackResult> {
        match ev {
            NetToolsEvent::RouteLookupAdded { lookup } => {
                println!(
                    "route lookup added: {} via {} dev {}",
                    lookup.target, lookup.route.gateway, lookup.route.iface
                );
                Some(serde_json::json!({ "event": "route_lookup_added", "lookup": lookup }))
            }
            NetToolsEvent::RouteLookupRemoved { lookup } => {
                println!(
                    "route lookup removed: {} via {} dev {}",
                    lookup.target, lookup.route.gateway, lookup.route.iface
                );
                Some(serde_json::json!({ "event": "route_lookup_removed", "lookup": lookup }))
            }
            NetToolsEvent::RouteLookupChanged { old, new } => {
                println!(
                    "route lookup changed: {} via {} dev {} -> via {} dev {}",
                    old.target, old.route.gateway, old.route.iface, new.route.gateway, new.route.iface
                );
                Some(serde_json::json!({ "event": "route_lookup_changed", "old": old, "new": new }))
            }
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() {
    let mut sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_secs(2))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .nethealth(false)
                .sockets(false)
                .neighbours(false)
                .route_lookups(true),
        ),
    );

    let targets = env::args().skip(1).collect::<Vec<_>>();
    if targets.is_empty() {
        sensor.add_route_lookup_target("8.8.8.8");
        sensor.add_route_lookup_target("2001:4860:4860::8888");
    } else {
        targets.into_iter().for_each(|target| sensor.add_route_lookup_target(target));
    }

    let (tx, mut rx) = channel::<CallbackResult>(0xff);

    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);
    let hub = Arc::new(hub);

    let rx_task = tokio::spawn(async move {
        while let Some(result) = rx.recv().await {
            println!("RESULT: {result}");
        }
    });

    let (handle, mut sensor_task) = spawn_sensor(sensor, hub);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\nStopping...");
            handle.shutdown();
        }
        _ = &mut sensor_task => {}
    }

    let _ = sensor_task.await;
    rx_task.abort();
}
