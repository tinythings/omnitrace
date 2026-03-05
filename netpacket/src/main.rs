use async_trait::async_trait;
use netpacket::events::{NetNotifyEvent, NetNotifyMask};
use netpacket::{NetNotify, NetNotifyConfig};
use omnitrace_core::callbacks::{Callback, CallbackHub, CallbackResult};
use omnitrace_core::sensor::spawn_sensor;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::channel;

struct JsonCb;

#[async_trait]
impl Callback<NetNotifyEvent> for JsonCb {
    fn mask(&self) -> u64 {
        (NetNotifyMask::OPENED | NetNotifyMask::CLOSED).bits()
    }

    async fn call(&self, ev: &NetNotifyEvent) -> Option<CallbackResult> {
        let (evname, conn) = match ev {
            NetNotifyEvent::Opened { conn } => ("opened", conn),
            NetNotifyEvent::Closed { conn } => ("closed", conn),
        };

        let remote_pretty = match (&conn.remote_dec, &conn.remote_host) {
            (Some(ipport), Some(host)) => format!("{ipport} ({host})"),
            (Some(ipport), None) => ipport.clone(),
            _ => "-".to_string(),
        };

        println!(
            "{} {} -> {} [{}:{}]",
            evname,
            conn.local_dec.as_deref().unwrap_or("-"),
            remote_pretty,
            conn.proto,
            conn.state_dec.as_deref().unwrap_or("-"),
        );

        Some(serde_json::json!({
            "event": evname,
            "conn": {
                "proto": conn.proto,
                "local_raw": conn.local,
                "remote_raw": conn.remote,
                "local": conn.local_dec,
                "remote": conn.remote_dec,
                "remote_host": conn.remote_host,
                "state": conn.state_dec,
                "remote_sni": conn.remote_sni,
            }
        }))
    }
}

#[tokio::main]
async fn main() {
    // Demo:
    // SNI_IFACE=eth0 cargo run -p netpacket
    // If unset, SNI sniffer runs in auto mode (UP non-loopback interfaces).
    let mut cfg = NetNotifyConfig::default().pulse(Duration::from_secs(1));
    if let Ok(iface) = env::var("SNI_IFACE")
        && !iface.trim().is_empty()
    {
        println!("SNI capture interface: {}", iface);
        cfg = cfg.sni_interface(iface);
    } else {
        println!("SNI capture interface: auto (all UP non-loopback interfaces)");
    }

    let mut sensor = NetNotify::new(Some(cfg)).dns(true).dns_ttl(Duration::from_secs(5));

    // Rule:
    // - add("*.google.com") => turns on reverse DNS + matches by hostname (glob)
    // - add("1.2.3.4") or add("1.2.3.4:443") => matches IP (no DNS needed)
    // - add("*") => “watch everything” (aka: eyeball cancer)
    // sensor.add("*.google.com");
    // sensor.add("8.8.8.8"); // IP-only filter example
    sensor.add("*"); // if you hate yourself
    sensor.ignore("udp * *"); // optional noise filter

    let (tx, mut rx) = channel::<CallbackResult>(0xfff);

    let mut hub = CallbackHub::<NetNotifyEvent>::new();
    hub.add(JsonCb);
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
