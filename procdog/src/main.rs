use procdog::{
    ProcDog, ProcDogConfig,
    events::{Callback, EventMask},
};
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let mut dog = ProcDog::new(Some(
        ProcDogConfig::default().interval(Duration::from_secs(1)),
    ));
    dog.watch("bash"); // or any other different shell, or "python", etc.

    let cb = Callback::new(EventMask::APPEARED | EventMask::DISAPPEARED).on(|ev| async move {
        println!("EVENT: {:?}", ev);
        None
    });

    dog.add_callback(cb);

    let (tx, mut rx) = mpsc::channel(0xff);
    dog.set_callback_channel(tx);

    tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {}", r);
        }
    });

    // Run sensor in background
    tokio::spawn(dog.run());

    // Simulate your app doing other work
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("App is doing other work...");
    }
}
