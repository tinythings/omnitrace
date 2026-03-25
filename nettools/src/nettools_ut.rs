use crate::{
    HostnameBackend, NeighbourBackend, NetHealthBackend, NetTools, NetToolsConfig, RouteBackend, SocketBackend,
    events::{
        NetHealthLevel, NetHealthTarget, NetToolsEvent, NeighbourEntry, RouteEntry, RouteFamily, SocketEntry,
        SocketKind,
    },
};
use async_trait::async_trait;
use omnitrace_core::{
    callbacks::{Callback, CallbackHub, CallbackResult},
    sensor::spawn_sensor,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
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

struct SequenceSocketBackend {
    values: Mutex<VecDeque<io::Result<HashSet<SocketEntry>>>>,
    last: Mutex<Option<HashSet<SocketEntry>>>,
}

struct SequenceNeighbourBackend {
    values: Mutex<VecDeque<io::Result<HashMap<String, NeighbourEntry>>>>,
    last: Mutex<Option<HashMap<String, NeighbourEntry>>>,
}

impl SequenceNeighbourBackend {
    fn new(values: Vec<io::Result<HashMap<String, NeighbourEntry>>>) -> Self {
        Self { values: Mutex::new(values.into()), last: Mutex::new(None) }
    }
}

impl NeighbourBackend for SequenceNeighbourBackend {
    fn list(&self) -> io::Result<HashMap<String, NeighbourEntry>> {
        if let Some(value) = self.values.lock().unwrap().pop_front() {
            if let Ok(neighbours) = &value {
                *self.last.lock().unwrap() = Some(neighbours.clone());
            }

            return value;
        }

        Ok(self.last.lock().unwrap().clone().unwrap_or_default())
    }
}

impl SequenceSocketBackend {
    fn new(values: Vec<io::Result<HashSet<SocketEntry>>>) -> Self {
        Self { values: Mutex::new(values.into()), last: Mutex::new(None) }
    }
}

impl SocketBackend for SequenceSocketBackend {
    fn list(&self) -> io::Result<HashSet<SocketEntry>> {
        if let Some(value) = self.values.lock().unwrap().pop_front() {
            if let Ok(sockets) = &value {
                *self.last.lock().unwrap() = Some(sockets.clone());
            }

            return value;
        }

        Ok(self.last.lock().unwrap().clone().unwrap_or_default())
    }
}

struct SequenceNetHealthBackend {
    values: Mutex<VecDeque<io::Result<Duration>>>,
}

impl SequenceNetHealthBackend {
    fn new(values: Vec<io::Result<Duration>>) -> Self {
        Self { values: Mutex::new(values.into()) }
    }
}

#[async_trait]
impl NetHealthBackend for SequenceNetHealthBackend {
    async fn probe(&self, _target: &NetHealthTarget, _probe_timeout: Duration) -> io::Result<Duration> {
        self.values
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(io::Error::new(io::ErrorKind::TimedOut, "no more probe data")))
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
            NetToolsEvent::NetHealthChanged { old, new } => {
                Some(serde_json::json!({ "event": "nethealth_changed", "old": old, "new": new }))
            }
            NetToolsEvent::SocketAdded { socket } => {
                Some(serde_json::json!({ "event": "socket_added", "socket": socket }))
            }
            NetToolsEvent::SocketRemoved { socket } => {
                Some(serde_json::json!({ "event": "socket_removed", "socket": socket }))
            }
            NetToolsEvent::NeighbourAdded { neighbour } => {
                Some(serde_json::json!({ "event": "neighbour_added", "neighbour": neighbour }))
            }
            NetToolsEvent::NeighbourRemoved { neighbour } => {
                Some(serde_json::json!({ "event": "neighbour_removed", "neighbour": neighbour }))
            }
            NetToolsEvent::NeighbourChanged { old, new } => {
                Some(serde_json::json!({ "event": "neighbour_changed", "old": old, "new": new }))
            }
        }
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

fn socket(proto: &str, local: &str, remote: &str, state: Option<&str>, kind: SocketKind) -> SocketEntry {
    SocketEntry {
        proto: proto.to_string(),
        local: local.to_string(),
        remote: remote.to_string(),
        state: state.map(str::to_string),
        kind,
    }
}

fn neighbour(address: &str, mac: &str, iface: &str, state: Option<&str>) -> NeighbourEntry {
    NeighbourEntry {
        address: address.to_string(),
        mac: mac.to_string(),
        iface: iface.to_string(),
        state: state.map(str::to_string),
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
    sensor.set_route_backend(SequenceRouteBackend::new(vec![
        Ok(vec![route("default", "10.0.0.1", "em0")]),
        Ok(vec![route("default", "10.0.0.1", "em0"), route("10.1.0.0/16", "10.0.0.2", "em0")]),
    ]));

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
    sensor.set_route_backend(SequenceRouteBackend::new(vec![
        Ok(vec![route("default", "10.0.0.1", "em0")]),
        Ok(vec![route("default", "10.0.0.254", "em1")]),
    ]));

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
    sensor.set_route_backend(SequenceRouteBackend::new(vec![
        Ok(vec![route("10.1.0.0/16", "10.0.0.2", "em0")]),
        Ok(vec![route("10.1.0.0/16", "10.0.0.2", "em0"), route("default", "10.0.0.1", "em0")]),
    ]));

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
    sensor.set_route_backend(SequenceRouteBackend::new(vec![
        Ok(vec![route("default", "10.0.0.1", "em0")]),
        Ok(vec![route("default", "10.0.0.254", "em1")]),
    ]));

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

#[tokio::test]
async fn emits_nethealth_changed_event_for_latency_spike() {
    let mut sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_millis(10))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .nethealth(true)
                .nethealth_window(1)
                .nethealth_latency_degraded_ms(200),
        ),
    );
    sensor.add_nethealth_target("probe.example", 443);
    sensor.set_nethealth_backend(SequenceNetHealthBackend::new(vec![
        Ok(Duration::from_millis(50)),
        Ok(Duration::from_millis(600)),
    ]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "nethealth_changed");
    assert_eq!(event["old"]["level"], serde_json::json!(NetHealthLevel::Healthy));
    assert_eq!(event["new"]["level"], serde_json::json!(NetHealthLevel::Degraded));
}

#[tokio::test]
async fn emits_nethealth_changed_event_for_connectivity_loss() {
    let mut sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_millis(10))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .nethealth(true)
                .nethealth_window(1),
        ),
    );
    sensor.add_nethealth_target("probe.example", 443);
    sensor.set_nethealth_backend(SequenceNetHealthBackend::new(vec![
        Ok(Duration::from_millis(50)),
        Err(io::Error::new(io::ErrorKind::TimedOut, "timeout")),
    ]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "nethealth_changed");
    assert_eq!(event["old"]["level"], serde_json::json!(NetHealthLevel::Healthy));
    assert_eq!(event["new"]["level"], serde_json::json!(NetHealthLevel::Down));
}

#[tokio::test]
async fn emits_socket_added_event_for_listener() {
    let mut sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_millis(10))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .sockets(true),
        ),
    );
    sensor.set_socket_backend(SequenceSocketBackend::new(vec![
        Ok(HashSet::new()),
        Ok(HashSet::from([socket("tcp", "0.0.0.0:22", "0.0.0.0:0", Some("LISTEN"), SocketKind::Listener)])),
    ]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "socket_added");
    assert_eq!(event["socket"]["kind"], serde_json::json!(SocketKind::Listener));
    assert_eq!(event["socket"]["local"], "0.0.0.0:22");
}

#[tokio::test]
async fn emits_socket_removed_event_for_connection() {
    let mut sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_millis(10))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .sockets(true),
        ),
    );
    sensor.set_socket_backend(SequenceSocketBackend::new(vec![
        Ok(HashSet::from([socket(
            "tcp",
            "10.0.0.5:54000",
            "10.0.0.10:443",
            Some("ESTABLISHED"),
            SocketKind::Connection,
        )])),
        Ok(HashSet::new()),
    ]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "socket_removed");
    assert_eq!(event["socket"]["kind"], serde_json::json!(SocketKind::Connection));
    assert_eq!(event["socket"]["remote"], "10.0.0.10:443");
}

#[tokio::test]
async fn emits_socket_added_event_for_ipv6_listener() {
    let mut sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_millis(10))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .sockets(true),
        ),
    );
    sensor.set_socket_backend(SequenceSocketBackend::new(vec![
        Ok(HashSet::new()),
        Ok(HashSet::from([socket("tcp6", "[::1]:443", "[::]:0", Some("LISTEN"), SocketKind::Listener)])),
    ]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "socket_added");
    assert_eq!(event["socket"]["proto"], "tcp6");
    assert_eq!(event["socket"]["local"], "[::1]:443");
}

#[tokio::test]
async fn emits_neighbour_added_event() {
    let mut sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_millis(10))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .neighbours(true),
        ),
    );
    sensor.set_neighbour_backend(SequenceNeighbourBackend::new(vec![
        Ok(HashMap::new()),
        Ok(HashMap::from([(
            "192.168.1.10".to_string(),
            neighbour("192.168.1.10", "aa:bb:cc:dd:ee:ff", "eth0", Some("0x2")),
        )])),
    ]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "neighbour_added");
    assert_eq!(event["neighbour"]["address"], "192.168.1.10");
    assert_eq!(event["neighbour"]["mac"], "aa:bb:cc:dd:ee:ff");
}

#[tokio::test]
async fn emits_neighbour_changed_event() {
    let mut sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_millis(10))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .neighbours(true),
        ),
    );
    sensor.set_neighbour_backend(SequenceNeighbourBackend::new(vec![
        Ok(HashMap::from([(
            "192.168.1.10".to_string(),
            neighbour("192.168.1.10", "aa:bb:cc:dd:ee:ff", "eth0", Some("0x2")),
        )])),
        Ok(HashMap::from([(
            "192.168.1.10".to_string(),
            neighbour("192.168.1.10", "11:22:33:44:55:66", "eth1", Some("0x6")),
        )])),
    ]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "neighbour_changed");
    assert_eq!(event["old"]["iface"], "eth0");
    assert_eq!(event["new"]["iface"], "eth1");
}

#[tokio::test]
async fn emits_neighbour_added_event_for_ipv6() {
    let mut sensor = NetTools::new(
        Some(
            NetToolsConfig::default()
                .pulse(Duration::from_millis(10))
                .hostname(false)
                .routes(false)
                .default_routes(false)
                .neighbours(true),
        ),
    );
    sensor.set_neighbour_backend(SequenceNeighbourBackend::new(vec![
        Ok(HashMap::new()),
        Ok(HashMap::from([(
            "fe80::1".to_string(),
            neighbour("fe80::1", "aa:bb:cc:dd:ee:ff", "eth0", Some("REACHABLE")),
        )])),
    ]));

    let (tx, mut rx) = channel::<CallbackResult>(4);
    let mut hub = CallbackHub::<NetToolsEvent>::new();
    hub.add(JsonCb);
    hub.set_result_channel(tx);

    let (handle, sensor_task) = spawn_sensor(sensor, Arc::new(hub));
    let event = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();

    handle.shutdown();
    let _ = sensor_task.await;

    assert_eq!(event["event"], "neighbour_added");
    assert_eq!(event["neighbour"]["address"], "fe80::1");
    assert_eq!(event["neighbour"]["state"], "REACHABLE");
}
