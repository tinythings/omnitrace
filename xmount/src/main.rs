use std::time::Duration;
use tokio::sync::mpsc::channel;
use xmount::events::{Callback, EventMask, XMountEvent};
use xmount::{XMount, XMountConfig};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut x = XMount::new(XMountConfig::default().pulse(Duration::from_millis(500)));

    x.add("/mnt/your-usb-drive");
    x.add("/media/somedisk");

    let cb = Callback::new(EventMask::MOUNTED | EventMask::UNMOUNTED | EventMask::CHANGED).on(|ev| async move {
        match ev {
            XMountEvent::Mounted { target, info } => {
                println!("MOUNTED: {:?} <- {} ({})", target, info.source, info.fstype);
                Some(serde_json::json!({
                    "event": "mounted",
                    "target": target.to_string_lossy(),
                    "source": info.source,
                    "fstype": info.fstype,
                    "opts": info.mount_opts,
                }))
            }
            XMountEvent::Unmounted { target, last } => {
                println!("UNMOUNTED: {:?} (was {} {})", target, last.source, last.fstype);
                Some(serde_json::json!({
                    "event": "unmounted",
                    "target": target.to_string_lossy(),
                    "last_source": last.source,
                    "last_fstype": last.fstype,
                }))
            }
            XMountEvent::Changed { target, old, new } => {
                println!(
                    "CHANGED: {:?} {}:{} -> {}:{}",
                    target, old.source, old.fstype, new.source, new.fstype
                );
                Some(serde_json::json!({
                    "event": "changed",
                    "target": target.to_string_lossy(),
                    "old": { "source": old.source, "fstype": old.fstype, "opts": old.mount_opts },
                    "new": { "source": new.source, "fstype": new.fstype, "opts": new.mount_opts },
                }))
            }
        }
    });

    x.add_callback(cb);

    let (tx, mut rx) = channel::<serde_json::Value>(0xfff);
    x.set_callback_channel(tx);
    tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {}", r);
        }
    });

    tokio::spawn(async move {
        // runs forever unless /proc explodes
        let _ = x.run().await;
    });

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("App is doing other work... (pretending to be useful)");
    }
}
