use async_trait::async_trait;
use omnitrace_core::callbacks::{Callback, CallbackResult};
use procdog::{
    ProcDog, ProcDogConfig,
    events::{ProcDogEvent, ProcDogMask},
};
use std::time::Duration;
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

    dog.add_callback(PrintCb);

    let (tx, mut rx) = mpsc::channel::<CallbackResult>(0xff);
    dog.set_callback_channel(tx);

    tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {}", r);
        }
    });

    tokio::spawn(dog.run());

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("App is doing other work...");
    }
}
