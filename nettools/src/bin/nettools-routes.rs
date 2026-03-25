use async_trait::async_trait;
use nettools::events::{NetToolsEvent, NetToolsMask};
use nettools::{NetTools, NetToolsConfig};
use omnitrace_core::callbacks::{Callback, CallbackHub, CallbackResult};
use omnitrace_core::sensor::spawn_sensor;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::channel;

struct JsonCb;

#[async_trait]
impl Callback<NetToolsEvent> for JsonCb {
    fn mask(&self) -> u64 {
        (NetToolsMask::ROUTE_ADDED | NetToolsMask::ROUTE_REMOVED | NetToolsMask::ROUTE_CHANGED).bits()
    }

    async fn call(&self, ev: &NetToolsEvent) -> Option<CallbackResult> {
        match ev {
            NetToolsEvent::RouteAdded { route } => {
                println!("route added: {} via {} dev {}", route.destination, route.gateway, route.iface);
                Some(serde_json::json!({
                    "event": "route_added",
                    "route": route,
                }))
            }
            NetToolsEvent::RouteRemoved { route } => {
                println!("route removed: {} via {} dev {}", route.destination, route.gateway, route.iface);
                Some(serde_json::json!({
                    "event": "route_removed",
                    "route": route,
                }))
            }
            NetToolsEvent::RouteChanged { old, new } => {
                println!("route changed: {} via {} dev {} -> via {} dev {}", old.destination, old.gateway, old.iface, new.gateway, new.iface);
                Some(serde_json::json!({
                    "event": "route_changed",
                    "old": old,
                    "new": new,
                }))
            }
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() {
    let sensor = NetTools::new(Some(NetToolsConfig::default().pulse(Duration::from_secs(2)).hostname(false).routes(true)));
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
