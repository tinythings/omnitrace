# FileScream

**FileScream** is a userspace file-change detector that behaves like
`fanotify` / `inotify` — without turning your system into a brick when
you point it at large trees like `/usr`.

It uses **intelligent metadata polling** instead of kernel event queues,
which makes it portable, predictable, and immune to the usual watcher
failure modes. In this case, if the kernel doesn’t support notifications,
FileScream still works.


## What it is
- A **portable file and directory change detector**
- Fully **userspace-based**
- Designed for **embedded, constrained, and cross-OS environments**
- Async-friendly (Tokio-based), no runtime ownership assumptions


## What it is *not*
- Not a kernel subsystem
- Not a thin wrapper around `inotify`
- Not event-queue magic that explodes under load

### Usage Example

Here is a simple example how to use this:

```rust
// Create a watcher
let mut fs = FileScream::new(Some(FileScriptConfig::default().pulse(Duration::from_secs(1))));

// Tell what to watch
fs.watch("/my/path");

// Define a callback
let cb = Callback::new(EventMask::CREATED).on(|ev| async move {
    match ev {
        FileScreamEvent::Created { path } => {
            println!("File has been created: {:?}", path);
            Some(serde_json::json!({ "event": "created", "path": path.to_string_lossy() }))
        }
    _ => None,
    }
});
fs.add_callback(cb);

// Setup a channel and start receiving data
let (tx, mut rx) = tokio::sync::mpsc::channel::<serde_json::Value>(0xfff);
fs.set_callback_channel(tx);
tokio::spawn(async move {
    while let Some(r) = rx.recv().await {
        println!("RESULT: {}", r);
    }
});

// Begin listening
tokio::spawn(fs.run());
```

Basically, just that.

## Pros
- Works on **every Unix-like OS** (Linux, BSDs, QNX, etc.)
- Works on **any filesystem**, even those without notification support
- No kernel limits or queue overflows. And watcher count doesn't explode
- No fallback paths, just one deterministic mechanism
- No external services or third-party daemons


## Cons
Sorry, since physics still applies:
- Userspace-based, so it has *some* overhead
- As it is polling-based, it *theoretically* less CPU-efficient than perfect kernel events
  *(in practice often **more** predictable and stable)*

## TODOs (next steps)

- [ ] Symlink watches
- [ ] File attribute changes, permissions
- [ ] File renames/moves

## Why FileScream exists
Kernel file notification APIs are:
- platform-specific
- fragile under scale
- inconsistent across filesystems
- hostile to embedded and long-running systems

FileScream trades instant kernel events for:
- correctness
- portability
- bounded resource usage
- sane behavior under real-world load
