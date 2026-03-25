use crate::{HostnameBackend, NetTools, NetToolsConfig, RouteBackend, events::{NetToolsEvent, RouteEntry, RouteFamily}};
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
            NetToolsEvent::RouteAdded { route } => {
                Some(serde_json::json!({ "event": "route_added", "route": route }))
            }
            NetToolsEvent::RouteRemoved { route } => {
                Some(serde_json::json!({ "event": "route_removed", "route": route }))
            }
            NetToolsEvent::RouteChanged { old, new } => {
                Some(serde_json::json!({ "event": "route_changed", "old": old, "new": new }))
            }
            NetToolsEvent::DefaultRouteAdded { route } => {
                Some(serde_json::json!({ "event": "default_route_added", "route": route }))
            }
            NetToolsEvent::DefaultRouteRemoved { route } => {
                Some(serde_json::json!({ "event": "default_route_removed", "route": route }))
            }
            NetToolsEvent::DefaultRouteChanged { old, new } => {
                Some(serde_json::json!({ "event": "default_route_changed", "old": old, "new": new }))
            }
        }
    }
}

struct SequenceRouteBackend {
    values: Mutex<VecDeque<io::Result<Vec<RouteEntry>>>>,
    last: Mutex<Option<Vec<RouteEntry>>>,
}

impl SequenceRouteBackend {
    fn new(values: Vec<io::Result<Vec<RouteEntry>>>) -> Self {
        Self { values: Mutex::new(values.into()), last: Mutex::new(None) }
    }
}

impl RouteBackend for SequenceRouteBackend {
    fn list(&self) -> io::Result<Vec<RouteEntry>> {
        if let Some(value) = self.values.lock().unwrap().pop_front() {
            if let Ok(routes) = &value {
                *self.last.lock().unwrap() = Some(routes.clone());
            }

            return value;
        }

        Ok(self.last.lock().unwrap().clone().unwrap_or_default())
    }
}

fn route(destination: &str, gateway: &str, iface: &str) -> RouteEntry {
    RouteEntry {
        family: RouteFamily::Inet,
        destination: destination.to_string(),
        gateway: gateway.to_string(),
        iface: iface.to_string(),
    }
}

#[tokio::test]
async fn emits_hostname_changed_event() {
    let mut sensor = NetTools::new(Some(NetToolsConfig::default().pulse(Duration::from_millis(10))));
    sensor.set_hostname_backend(SequenceBackend::new(vec![Ok("alpha"), Ok("beta")]));

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
    sensor.set_hostname_backend(SequenceBackend::new(vec![Ok("alpha"), Ok("alpha"), Ok("alpha")]));

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

#[tokio::test]
async fn emits_route_added_event() {
    let mut sensor = NetTools::new(Some(NetToolsConfig::default().pulse(Duration::from_millis(10)).hostname(false).routes(true)));
    sensor.set_route_backend(SequenceRouteBackend::new(vec![Ok(vec![route("default", "10.0.0.1", "em0")]), Ok(vec![route("default", "10.0.0.1", "em0"), route("10.1.0.0/16", "10.0.0.2", "em0")])]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "route_added");
    assert_eq!(event["route"]["destination"], "10.1.0.0/16");
}

#[tokio::test]
async fn emits_route_changed_event() {
    let mut sensor = NetTools::new(Some(NetToolsConfig::default().pulse(Duration::from_millis(10)).hostname(false).routes(true)));
    sensor.set_route_backend(SequenceRouteBackend::new(vec![Ok(vec![route("default", "10.0.0.1", "em0")]), Ok(vec![route("default", "10.0.0.254", "em1")])]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "route_changed");
    assert_eq!(event["old"]["gateway"], "10.0.0.1");
    assert_eq!(event["new"]["gateway"], "10.0.0.254");
}

#[tokio::test]
async fn emits_default_route_added_event() {
    let mut sensor = NetTools::new(
        Some(NetToolsConfig::default().pulse(Duration::from_millis(10)).hostname(false).routes(false).default_routes(true)),
    );
    sensor.set_route_backend(SequenceRouteBackend::new(vec![Ok(vec![route("10.1.0.0/16", "10.0.0.2", "em0")]), Ok(vec![route("10.1.0.0/16", "10.0.0.2", "em0"), route("default", "10.0.0.1", "em0")])]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "default_route_added");
    assert_eq!(event["route"]["destination"], "default");
}

#[tokio::test]
async fn emits_default_route_changed_event() {
    let mut sensor = NetTools::new(
        Some(NetToolsConfig::default().pulse(Duration::from_millis(10)).hostname(false).routes(false).default_routes(true)),
    );
    sensor.set_route_backend(SequenceRouteBackend::new(vec![Ok(vec![route("default", "10.0.0.1", "em0")]), Ok(vec![route("default", "10.0.0.254", "em1")])]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "default_route_changed");
    assert_eq!(event["old"]["gateway"], "10.0.0.1");
    assert_eq!(event["new"]["gateway"], "10.0.0.254");
}
