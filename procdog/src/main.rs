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

    // Set a proper backend for your platform (optional, will auto-detect)
    #[cfg(target_os = "linux")]
    dog.set_backend(procdog::backends::linuxps::LinuxPsBackend);

    #[cfg(target_os = "netbsd")]
    dog.set_backend(procdog::backends::netbsd_sysctl::NetBsdSysctlBackend);

    #[cfg(all(not(target_os = "linux"), not(target_os = "netbsd")))]
    dog.set_backend(procdog::backends::stps::PsBackend);

    // Add processes to watch (you can add/remove at runtime)
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
