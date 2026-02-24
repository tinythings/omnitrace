# xmount

Tiny Rust library that monitors Linux mountpoints.

You register mountpoints to watch, attach async callbacks, and it emits events when a mountpoint is:

- **Mounted**
- **Unmounted**
- **Changed** (mount ID/source/fs/options/etc.)

Built for simple, deterministic behavior. No inotify. No magic. Just polling.

## Features

- Watches specific mount targets (`/mnt/usb`, `/run/media/...`, etc.)
- Async callbacks with event masks
- Optional channel for callback results
- Handles mountinfo escaping (`\040`, etc.)
- Minimal dependencies (Tokio for the loop)

## Quick example

```rust
use xmount::{XMount, XMountConfig};
use xmount::events::{XMountCallback, XMountEvent, XMountMask};
use std::time::Duration;

struct PrintCb;

#[async_trait::async_trait]
impl XMountCallback for PrintCb {
    fn mask(&self) -> XMountMask {
        XMountMask::MOUNTED | XMountMask::UNMOUNTED | XMountMask::CHANGED
    }

    async fn call(&self, ev: &XMountEvent) -> Option<xmount::events::CallbackResult> {
        println!("{ev:?}");
        None
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cfg = XMountConfig::default().pulse(Duration::from_secs(1));
    let mut xm = XMount::new(cfg);

    xm.add("/mnt/usb");
    xm.add_callback(PrintCb);

    xm.run().await
}
