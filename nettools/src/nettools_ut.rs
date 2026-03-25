use crate::{HostnameBackend, NetTools, NetToolsConfig, events::NetToolsEvent};
use async_trait::async_trait;
use omnitrace_core::{
    callbacks::{Callback, CallbackHub, CallbackResult},
    sensor::spawn_sensor,
};
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc::channel;

struct SequenceBackend {
    values: Mutex<VecDeque<io::Result<String>>>,
    last: Mutex<Option<String>>,
}

impl SequenceBackend {
    fn new(values: Vec<io::Result<&str>>) -> Self {
        Self {
            values: Mutex::new(values.into_iter().map(|value| value.map(|value| value.to_string())).collect()),
            last: Mutex::new(None),
        }
    }
}

impl HostnameBackend for SequenceBackend {
    fn current(&self) -> io::Result<String> {
        if let Some(value) = self.values.lock().unwrap().pop_front() {
            if let Ok(hostname) = &value {
                *self.last.lock().unwrap() = Some(hostname.clone());
            }
            return value;
        }

        Ok(self.last.lock().unwrap().clone().unwrap_or_else(|| "stable-host".to_string()))
    }
}

struct JsonCb;

#[async_trait]
impl Callback<NetToolsEvent> for JsonCb {
    fn mask(&self) -> u64 {
        u64::MAX
    }

    async fn call(&self, ev: &NetToolsEvent) -> Option<CallbackResult> {
        match ev {
            NetToolsEvent::HostnameChanged { old, new } => {
                Some(serde_json::json!({ "event": "hostname_changed", "old": old, "new": new }))
            }
        }
    }
}

#[tokio::test]
async fn emits_hostname_changed_event() {
    let mut sensor = NetTools::new(Some(NetToolsConfig::default().pulse(Duration::from_millis(10))));
    sensor.set_backend(SequenceBackend::new(vec![Ok("alpha"), Ok("beta")]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "hostname_changed");
    assert_eq!(event["old"], "alpha");
    assert_eq!(event["new"], "beta");
}

#[tokio::test]
async fn does_not_emit_when_hostname_is_unchanged() {
    let mut sensor = NetTools::new(Some(NetToolsConfig::default().pulse(Duration::from_millis(10))));
    sensor.set_backend(SequenceBackend::new(vec![Ok("alpha"), Ok("alpha"), Ok("alpha")]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(80), rx.recv()).await;

    handle.shutdown();
    let _ = sensor_task.await;

    assert!(event.is_err());
}
