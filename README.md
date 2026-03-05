# Omnitrace

Omnitrace is a Rust-based sensor collection framework for low-level system observation.

It provides focused, event-driven sensors for filesystem, process, and mount monitoring — designed to be predictable, cross-platform where possible, and free of hidden background daemons.

This repository is a monorepo containing multiple sensor crates and a shared core.

## Sensors

### xmount
Mount table monitoring.

- Linux: `/proc/self/mountinfo`
- NetBSD: `getmntinfo(3)` / `statvfs`
- Events:
  - Mounted
  - Unmounted
  - Changed

Polling-based, deterministic behavior.

### procdog
Process monitoring sensor.

Backend-based design:
- Linux: `/proc`
- NetBSD: `sysctl`

Emits lifecycle-style process events.

### socktray
Socket activity monitoring sensor.

- Linux/Android: `/proc/net/*` snapshot diffing
- NetBSD: `sysctl` PCB lists (libc)
- FreeBSD: `kinfo_getfile()` via libutil/libc
- Other targets: `netstat -an` fallback backend
- Events:
  - Opened
  - Closed

### iface
Network interface/link/address monitoring sensor.

- Linux/Android: `rtnetlink`
- NetBSD/FreeBSD: `PF_ROUTE` trigger + snapshot diff
- Events:
  - IfaceAdded / IfaceRemoved
  - LinkUp / LinkDown
  - AddrAdded / AddrRemoved

### filescream
Filesystem watcher.

Designed for event-driven file change detection with a unified callback system.


## Design Reasoning

- No hidden daemons
- No implicit retries
- No global state
- Explicit event flow
- Deterministic behavior
- Portable, cross-platform (except Windows, but who cares 😉)

Each sensor is independent but shares the same callback model.

---

## Building

Build everything:

```bash
cargo build
```

Check workspace:

```bash
cargo check
```

Build a specific crate:

```bash
cargo build -p <name>
```

## Test From CLI (socktray)

Run the sensor:

```bash
cargo run -p socktray
```

In another terminal, generate socket activity:

Linux/Android:

```bash
curl -I https://example.com
nc -vz 1.1.1.1 443
```

NetBSD/FreeBSD:

```bash
fetch -qo - https://example.com > /dev/null
nc -vz 1.1.1.1 443
```

You should see `opened` / `closed` lines in the `socktray` terminal.

## Test From CLI (iface)

Run the sensor:

```bash
cargo run -p iface
```

In another terminal, trigger interface/link/address events.

Linux/Android:

```bash
ip link add dummy0 type dummy
ip link set dummy0 up
ip addr add 10.123.45.1/24 dev dummy0
ip addr del 10.123.45.1/24 dev dummy0
ip link del dummy0
```

NetBSD/FreeBSD:

```bash
ifconfig lo1 create
ifconfig lo1 up
ifconfig lo1 inet 10.123.45.1/24 alias
ifconfig lo1 inet 10.123.45.1 delete
ifconfig lo1 destroy
```

You should see `IfaceAdded/Removed`, `LinkUp/Down`, and `AddrAdded/Removed` lines in the `iface` terminal.

---

## Callback Model

All sensors use the same pattern:

```rust
use omnitrace_core::callbacks::{Callback, CallbackResult};

#[async_trait::async_trait]
impl Callback<MyEvent> for MyHandler {
    fn mask(&self) -> u64 { ... }

    async fn call(&self, ev: &MyEvent) -> Option<CallbackResult> {
        ...
    }
}
```

Events are filtered by bitmask before invocation.
Optional result channel allows sensors to emit structured JSON.

---

## Platform Support

Currently the main focus is Linux and NetBSD.

| Sensor      | Linux | NetBSD |
|-------------|-------|--------|
| xmount      | ✔     | ✔      |
| procdog     | ✔     | ✔      |
| iface       | ✔     | ✔      |
| socktray    | ✔     | ✔      |
| filescream  | ✔     | (planned) |

---
