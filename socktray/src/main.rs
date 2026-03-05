use async_trait::async_trait;
use omnitrace_core::callbacks::{Callback, CallbackHub, CallbackResult};
use omnitrace_core::sensor::spawn_sensor;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::channel;

use socktray::events::{SockTrayEvent, SockTrayMask};
use socktray::{SockTray, SockTrayConfig};

struct PrintCb;

#[async_trait]
impl Callback<SockTrayEvent> for PrintCb {
    fn mask(&self) -> u64 {
        (SockTrayMask::OPENED | SockTrayMask::CLOSED).bits()
    }

    async fn call(&self, ev: &SockTrayEvent) -> Option<CallbackResult> {
        let (kind, s) = match ev {
            SockTrayEvent::Opened { sock } => ("opened", sock),
            SockTrayEvent::Closed { sock } => ("closed", sock),
        };

        println!(
            "{} {} {} -> {} [{}]",
            kind,
            s.proto,
            s.local_dec.as_deref().unwrap_or(&s.local),
            s.remote_dec.as_deref().unwrap_or(&s.remote),
            s.state_dec.as_deref().or(s.state.as_deref()).unwrap_or("-")
        );

        None
    }
}

#[tokio::main]
async fn main() {
    let mut sensor = SockTray::new(Some(SockTrayConfig::default().pulse(Duration::from_secs(1))));
    sensor.add("*");
    sensor.ignore("udp * * *");

    let (tx, mut rx) = channel::<CallbackResult>(0xfff);

    let mut hub = CallbackHub::<SockTrayEvent>::new();
    hub.add(PrintCb);
    hub.set_result_channel(tx);
    let hub = Arc::new(hub);

    let rx_task = tokio::spawn(async move {
        while let Some(r) = rx.recv().await {
            println!("RESULT: {r}");
        }
    });

    let (handle, mut sensor_task) = spawn_sensor(sensor, hub.clone());

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\nStopping...");
            handle.shutdown();
        }
        _ = &mut sensor_task => {}
    }

    let _ = sensor_task.await;
    rx_task.abort();
}
