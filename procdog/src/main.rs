use async_trait::async_trait;
use omnitrace_core::{
    callbacks::{Callback, CallbackHub, CallbackResult},
    sensor::spawn_sensor,
};
use procdog::{
    ProcDog, ProcDogConfig,
    events::{ProcDogEvent, ProcDogMask},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

struct PrintCb;

#[async_trait]
impl Callback<ProcDogEvent> for PrintCb {
    fn mask(&self) -> u64 {
        (ProcDogMask::APPEARED | ProcDogMask::MISSING | ProcDogMask::DISAPPEARED).bits()
    }

    async fn call(&self, ev: &ProcDogEvent) -> Option<CallbackResult> {
        println!("EVENT: {:?}", ev);
        None
    }
}

#[tokio::main]
async fn main() {
    let mut dog = ProcDog::new(Some(ProcDogConfig::default().interval(Duration::from_secs(1)).emit_on_start(true)));

    // Set a proper backend for your platform (optional)
    #[cfg(target_os = "linux")]
    dog.set_backend(procdog::backends::linuxps::LinuxPsBackend);

    #[cfg(target_os = "netbsd")]
    dog.set_backend(procdog::backends::netbsd_sysctl::NetBsdSysctlBackend);

    #[cfg(all(not(target_os = "linux"), not(target_os = "netbsd")))]
    dog.set_backend(procdog::backends::stps::PsBackend);

    dog.watch("perl");

    let (tx, mut rx) = mpsc::channel::<CallbackResult>(0xff);

    let mut hub = CallbackHub::<ProcDogEvent>::new();
    hub.add(PrintCb);
    hub.set_result_channel(tx);
    let hub = Arc::new(hub);

    let rx_task = tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {}", r);
        }
    });

    let (handle, mut sensor_task) = spawn_sensor(dog, hub.clone());

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
}
