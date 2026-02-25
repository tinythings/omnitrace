# ProcDog

**ProcDog** is a cross-platform userspace process detector.
Just like **FileScream**, it is also used in the **Sysinspect** project as part of
the event listeners (sensors) facility.

All these sensors are similar in code and structure, but they intentionally do not
share code between them. Their usage, API and style are preserved to stay
more/less consistent. 😊

## What it is

ProcDog is a lightweight async process lifecycle watcher.

It:

- Periodically scans the system process list
- Tracks state transitions (running ↔ stopped)
- Detects PID changes (restart detection)
- Emits structured async events
- Supports callback + channel result model (FileScream-style)
- Works entirely in userspace

It does **not** depend on:

- systemd
- inotify
- /proc (hard dependency)
- platform-specific daemons
- kernel modules

The design goal is simplicity and portability first.

## What it is *not*

ProcDog is not:

- A service manager
- A supervisor
- A process spawner
- A health checker
- A metrics collector
- A performance monitor
- A replacement for systemd / launchd / rc.d
- A kernel event subscriber

It does not (important!):

- Capture exit codes
- Hook into waitpid()
- Track child process trees
- Guarantee instant detection
- Replace proper service orchestration

It onlt detects process presence and that is all.

## Usage Example

Here is a simple example how to use this:

```rust
#[tokio::main]
async fn main() {
    let mut dog = ProcDog::new(Some(
        ProcDogConfig::default().interval(Duration::from_secs(1)),
    ));
    dog.watch("bash"); // or any other shell, or "python", etc.

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
```

Basically, just that.

## Pros

- Very small
- No OS-specific hard dependencies
- Async-first design
- Clear transition-based event model
- Same API style as FileScream
- Easy to embed into larger systems
- Predictable behavior

## Cons

- Polling-based (not instant). There is no systemd on embedded Linux or BSD, sorry.
- ps parsing is not perfect
- Name matching may detect multiple instances
- No deep process inspection
- No exit status information
- No built-in debounce or flap detection (yet)

## Features

- Watch processes by name
- Detect start / stop transitions
- Detect PID change (restart)
- Callback-based async handling
- Optional channel-based result forwarding
- Simple configuration
- Priming (no initial false-positive events)

## Why ProcDog exists

- Small, focused sensor
- Async
- Stateless externally, stateful internally
- Emits transitions, not noise
- Does one thing well
