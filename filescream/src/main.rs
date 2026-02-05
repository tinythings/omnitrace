use filescream::events::{Callback, EventMask, FileScreamEvent};
use filescream::{FileScream, FileScriptConfig};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let mut fs = FileScream::new(Some(FileScriptConfig::default().pulse(Duration::from_secs(1))));

    fs.watch("/tmp");
    fs.ignore("in*r/"); // E.g. ignore /tmp/inner/foo.txt but not /tmp/inner.txt

    // Callback: react to CREATED + REMOVED, print and return JSON
    let cb = Callback::new(EventMask::CREATED | EventMask::REMOVED).on(|ev| async move {
        match ev {
            FileScreamEvent::Created { path } => {
                println!("File has been created: {:?}", path);
                Some(serde_json::json!({ "event": "created", "path": path.to_string_lossy() }))
            }
            FileScreamEvent::Removed { path } => {
                println!("File has been removed: {:?}", path);
                Some(serde_json::json!({ "event": "removed", "path": path.to_string_lossy() }))
            }
            _ => None,
        }
    });
    fs.add_callback(cb);

    // Setup a channel to receive callback results (optional)
    // and spawn a task to print them
    let (tx, mut rx) = tokio::sync::mpsc::channel::<serde_json::Value>(0xfff);
    fs.set_callback_channel(tx);
    tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {}", r);
        }
    });

    // Start the file watcher (runs indefinitely)
    tokio::spawn(fs.run());

    // emulate your app doing other work
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("App is doing other work...");
    }
}
