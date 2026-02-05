# FileScream

**FileScream** is a userspace-level little library that acts like
`fanotify`/`inotify` on Linux, but doesn't turning your OS system
into a brick, while scanning the entire `/usr` for example.

It also doesn't cry if you are moving it to another OS, say BSD
or QNX.

Pros:
  - Works on every *nix OS equally
  - Works on every filesystem, even if it doesn't support notification
  - Doesn't have "fallback" design
  - Doesn't require external/3rd-party components

Cons:
  - Not kernel based. It has its overhead.
  - It is a poller, so it is technically less CPU efficient
