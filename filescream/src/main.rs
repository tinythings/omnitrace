use async_trait::async_trait;
use filescream::events::{FileScreamEvent, FileScreamMask};
use filescream::{FileScream, FileScreamConfig};
use omnitrace_core::callbacks::{Callback, CallbackHub, CallbackResult};
use omnitrace_core::sensor::spawn_sensor;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::channel;

struct PrintCb;

#[async_trait]
impl Callback<FileScreamEvent> for PrintCb {
    fn mask(&self) -> u64 {
        (FileScreamMask::CREATED | FileScreamMask::CHANGED | FileScreamMask::REMOVED).bits()
    }

    async fn call(&self, ev: &FileScreamEvent) -> Option<CallbackResult> {
        println!("EVENT: {:?}", ev);
        None
    }
}

#[tokio::main]
async fn main() {
    let (tx, mut rx) = channel::<CallbackResult>(0xfff);

    let mut hub = CallbackHub::<FileScreamEvent>::new();
    hub.add(PrintCb);
    hub.set_result_channel(tx);
    let hub = Arc::new(hub);

    let mut fs = FileScream::new(Some(FileScreamConfig::default().pulse(Duration::from_secs(1))));
    fs.watch("/tmp");
    fs.ignore("in*r/");

    let rx_task = tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {r}");
        }
    });

    let (handle, mut sensor_task) = spawn_sensor(fs, hub.clone());

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("Shutting down on Ctrl-C...");
            handle.shutdown()
        },
        _ = &mut sensor_task => {}
    }

    let _ = sensor_task.await;
    rx_task.abort();
    let _ = rx_task.await;
}
