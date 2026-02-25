use async_trait::async_trait;
use omnitrace_core::callbacks::{Callback, CallbackResult};
use serde_json::json;
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

    // Put real mount targets here
    x.add("/mnt/your-usb-drive");
    x.add("/media/somedisk");

    // Callback
    x.add_callback(JsonCb);

    // Callback results channel (prints returned JSON)
    let (tx, mut rx) = channel::<CallbackResult>(0xfff);
    x.set_callback_channel(tx);

    tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {r}");
        }
    });

    // Run watcher (forever)
    tokio::spawn(async move {
        if let Err(e) = x.run().await {
            eprintln!("xmount failed: {e}");
        }
    });

    // Pretend we're a real application
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("App is doing other work... (pretending to be useful)");
    }
}
