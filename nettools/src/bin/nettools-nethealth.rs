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
        NetToolsMask::NETHEALTH_CHANGED.bits()
    }

    async fn call(&self, ev: &NetToolsEvent) -> Option<CallbackResult> {
        match ev {
            NetToolsEvent::NetHealthChanged { old, new } => {
                println!(
                    "nethealth changed: {:?} avg={:?}ms loss={} -> {:?} avg={:?}ms loss={}",
                    old.level, old.avg_rtt_ms, old.loss_pct, new.level, new.avg_rtt_ms, new.loss_pct
                );
                Some(serde_json::json!({ "event": "nethealth_changed", "old": old, "new": new }))
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
                .nethealth(true)
                .nethealth_window(3),
        ),
    );
    sensor.add_nethealth_target("1.1.1.1", 53);
    sensor.add_nethealth_target("8.8.8.8", 53);

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
