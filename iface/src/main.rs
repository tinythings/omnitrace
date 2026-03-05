use async_trait::async_trait;
use iface::events::{IfaceEvent, IfaceMask};
use iface::{Iface, IfaceConfig};
use omnitrace_core::callbacks::{Callback, CallbackHub, CallbackResult};
use omnitrace_core::sensor::spawn_sensor;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::channel;

struct PrintCb;

#[async_trait]
impl Callback<IfaceEvent> for PrintCb {
    fn mask(&self) -> u64 {
        (
            IfaceMask::IFACE_ADDED
                | IfaceMask::IFACE_REMOVED
                | IfaceMask::LINK_UP
                | IfaceMask::LINK_DOWN
                | IfaceMask::ADDR_ADDED
                | IfaceMask::ADDR_REMOVED
        )
        .bits()
    }

    async fn call(&self, ev: &IfaceEvent) -> Option<CallbackResult> {
        println!("{ev:?}");
        None
    }
}

#[tokio::main]
async fn main() {
    // Demo:
    // 1) Run: cargo run -p iface
    //
    // 2) Trigger events in another terminal.
    //
    // Linux/Android:
    //   ip link add dummy0 type dummy
    //   ip link set dummy0 up
    //   ip addr add 10.123.45.1/24 dev dummy0
    //   ip addr del 10.123.45.1/24 dev dummy0
    //   ip link del dummy0
    //
    // NetBSD/FreeBSD:
    //   ifconfig lo1 create
    //   ifconfig lo1 up
    //   ifconfig lo1 inet 10.123.45.1/24 alias
    //   ifconfig lo1 inet 10.123.45.1 delete
    //   ifconfig lo1 destroy
    let sensor = Iface::new(Some(IfaceConfig::default().poll_timeout(Duration::from_millis(250))));

    let (tx, mut rx) = channel::<CallbackResult>(0xfff);

    let mut hub = CallbackHub::<IfaceEvent>::new();
    hub.add(PrintCb);
    hub.set_result_channel(tx);
    let hub = Arc::new(hub);

    let rx_task = tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {r}");
        }
    });

    let (handle, mut sensor_task) = spawn_sensor(sensor, hub.clone());

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
