use async_trait::async_trait;
use omnitrace_core::callbacks::{Callback, CallbackResult};
use std::time::Duration;
use tokio::sync::mpsc::channel;

use filescream::events::{FileScreamEvent, FileScreamMask};
use filescream::{FileScream, FileScreamConfig};

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
    let mut fs = FileScream::new(Some(FileScreamConfig::default().pulse(Duration::from_secs(1))));

    fs.watch("/tmp");
    fs.ignore("in*r/"); // example ignore

    fs.add_callback(PrintCb);

    let (tx, mut rx) = channel::<CallbackResult>(0xfff);
    fs.set_callback_channel(tx);

    tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {r}");
        }
    });

    tokio::spawn(fs.run());

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("App is doing other work... (pretending to be useful)");
    }
}
