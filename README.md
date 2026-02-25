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
| filescream  | ✔     | (planned) |

---
