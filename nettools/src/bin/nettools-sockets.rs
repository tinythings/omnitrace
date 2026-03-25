use async_trait::async_trait;
use nettools::events::{NetToolsEvent, NetToolsMask, SocketKind};
use nettools::{NetTools, NetToolsConfig};
use omnitrace_core::callbacks::{Callback, CallbackHub, CallbackResult};
use omnitrace_core::sensor::spawn_sensor;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::channel;

struct JsonCb;

#[async_trait]
impl Callback<NetToolsEvent> for JsonCb {
    fn mask(&self) -> u64 {
        (NetToolsMask::SOCKET_ADDED | NetToolsMask::SOCKET_REMOVED).bits()
    }

    async fn call(&self, ev: &NetToolsEvent) -> Option<CallbackResult> {
        match ev {
            NetToolsEvent::SocketAdded { socket } => {
                println!(
                    "socket added: {} {} {} -> {} state={}",
                    socket.proto,
                    match socket.kind {
                        SocketKind::Listener => "listener",
                        SocketKind::Connection => "connection",
                    },
                    socket.local,
                    socket.remote,
                    socket.state.as_deref().unwrap_or("-")
                );
                Some(serde_json::json!({ "event": "socket_added", "socket": socket }))
            }
            NetToolsEvent::SocketRemoved { socket } => {
                println!(
                    "socket removed: {} {} {} -> {} state={}",
                    socket.proto,
                    match socket.kind {
                        SocketKind::Listener => "listener",
                        SocketKind::Connection => "connection",
                    },
                    socket.local,
                    socket.remote,
                    socket.state.as_deref().unwrap_or("-")
                );
                Some(serde_json::json!({ "event": "socket_removed", "socket": socket }))
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
                .sockets(true),
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
