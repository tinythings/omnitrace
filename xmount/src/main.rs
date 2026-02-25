use async_trait::async_trait;
use omnitrace_core::callbacks::{Callback, CallbackHub, CallbackResult};
use omnitrace_core::sensor::spawn_sensor;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::channel;
use xmount::events::{XMountEvent, XMountMask};
use xmount::{XMount, XMountConfig};

struct JsonCb;

#[async_trait]
impl Callback<XMountEvent> for JsonCb {
    fn mask(&self) -> u64 {
        (XMountMask::MOUNTED | XMountMask::UNMOUNTED | XMountMask::CHANGED).bits()
    }

    async fn call(&self, ev: &XMountEvent) -> Option<CallbackResult> {
        match ev {
            XMountEvent::Mounted { target, info } => {
                println!("MOUNTED: {:?} <- {} ({})", target, info.source, info.fstype);
                Some(json!({
                    "event": "mounted",
                    "target": target.to_string_lossy().to_string(),
                    "source": info.source,
                    "fstype": info.fstype,
                    "opts": info.mount_opts,
                }))
            }
            XMountEvent::Unmounted { target, last } => {
                println!("UNMOUNTED: {:?} (was {} {})", target, last.source, last.fstype);
                Some(json!({
                    "event": "unmounted",
                    "target": target.to_string_lossy().to_string(),
                    "last_source": last.source,
                    "last_fstype": last.fstype,
                }))
            }
            XMountEvent::Changed { target, old, new } => {
                println!("CHANGED: {:?} {}:{} -> {}:{}", target, old.source, old.fstype, new.source, new.fstype);
                Some(json!({
                    "event": "changed",
                    "target": target.to_string_lossy().to_string(),
                    "old": { "source": old.source, "fstype": old.fstype, "opts": old.mount_opts },
                    "new": { "source": new.source, "fstype": new.fstype, "opts": new.mount_opts },
                }))
            }
        }
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut x = XMount::new(XMountConfig::default().pulse(Duration::from_millis(500)));
    x.add("/mnt/your-usb-drive");
    x.add("/media/somedisk");

    let (tx, mut rx) = channel::<CallbackResult>(0xfff);

    let mut hub = CallbackHub::<XMountEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);
    let hub = Arc::new(hub);

    let rx_task = tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {r}");
        }
    });

    let (handle, mut sensor_task) = spawn_sensor(x, hub.clone());

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down on Ctrl-C...");
            handle.shutdown()
        },
        _ = &mut sensor_task => {}
    }

    let _ = sensor_task.await;
    rx_task.abort();
    let _ = rx_task.await;
    Ok(())
}
