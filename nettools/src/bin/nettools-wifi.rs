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
        (NetToolsMask::WIFI_ADDED | NetToolsMask::WIFI_REMOVED | NetToolsMask::WIFI_CHANGED).bits()
    }

    async fn call(&self, ev: &NetToolsEvent) -> Option<CallbackResult> {
        match ev {
            NetToolsEvent::WifiAdded { wifi } => {
                println!(
                    "wifi added: {} quality={} signal={}dBm noise={}dBm",
                    wifi.iface, wifi.link_quality, wifi.signal_level_dbm, wifi.noise_level_dbm
                );
                Some(serde_json::json!({ "event": "wifi_added", "wifi": wifi }))
            }
            NetToolsEvent::WifiRemoved { wifi } => {
                println!(
                    "wifi removed: {} quality={} signal={}dBm noise={}dBm",
                    wifi.iface, wifi.link_quality, wifi.signal_level_dbm, wifi.noise_level_dbm
                );
                Some(serde_json::json!({ "event": "wifi_removed", "wifi": wifi }))
            }
            NetToolsEvent::WifiChanged { old, new } => {
                println!(
                    "wifi changed: {} quality={} signal={}dBm noise={}dBm -> quality={} signal={}dBm noise={}dBm",
                    old.iface,
                    old.link_quality,
                    old.signal_level_dbm,
                    old.noise_level_dbm,
                    new.link_quality,
                    new.signal_level_dbm,
                    new.noise_level_dbm
                );
                Some(serde_json::json!({ "event": "wifi_changed", "old": old, "new": new }))
            }
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() {
    let sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_secs(2))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .nethealth(false)
                .sockets(false)
                .neighbours(false)
                .route_lookups(false)
                .throughput(false)
                .wifi(true),
        ),
    );
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
