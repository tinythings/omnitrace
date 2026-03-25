# nettools demo binaries

This directory contains small demo binaries for the `nettools` crate.

They are not product logic. They exist so you can run one feature at a time and
 see what events it produces.

Available demos

- `nettools-hostchange`
  - Watches the live system hostname.
  - Prints an event when the hostname changes.

- `nettools-routes`
  - Watches the routing table.
  - Prints events when routes are added, removed, or changed.

How to run

Run the hostname demo:

```bash
cargo run -p nettools --bin nettools-hostchange
```

Run the routes demo:

```bash
cargo run -p nettools --bin nettools-routes
```

What to expect

`nettools-hostchange`

- Start the binary.
- Change the hostname in another shell.
- The demo prints a line such as:

```text
hostname changed: oldname -> newname
```

Examples for changing hostname

Linux with `hostnamectl`:

```bash
sudo hostnamectl set-hostname demo-host
sudo hostnamectl set-hostname old-host
```

Generic Unix with `hostname`:

```bash
sudo hostname demo-host
sudo hostname old-host
```

FreeBSD:

```bash
sudo hostname demo-host
sudo hostname old-host
```

NetBSD:

```bash
sudo hostname demo-host
sudo hostname old-host
```

OpenBSD:

```bash
doas hostname demo-host
doas hostname old-host
```

`nettools-routes`

- Start the binary.
- Add, delete, or change a route in another shell.
- The demo prints lines such as:

```text
route added: default via 192.168.1.1 dev em0
route removed: 10.0.0.0/24 via 10.0.0.1 dev em0
route changed: default via 192.168.1.1 dev em0 -> via 192.168.1.254 dev em1
```

Examples for route changes

Linux:

Add a route:

```bash
sudo ip route add 10.20.30.0/24 via 192.168.1.1 dev eth0
```

Delete a route:

```bash
sudo ip route del 10.20.30.0/24
```

Change or replace a route:

```bash
sudo ip route replace 10.20.30.0/24 via 192.168.1.254 dev eth0
```

Change default route:

```bash
sudo ip route replace default via 192.168.1.254 dev eth0
```

FreeBSD / NetBSD / OpenBSD:

Add a route:

```bash
sudo route add -net 10.20.30.0/24 192.168.1.1
```

Delete a route:

```bash
sudo route delete -net 10.20.30.0/24 192.168.1.1
```

Change a route:

```bash
sudo route delete -net 10.20.30.0/24 192.168.1.1
sudo route add -net 10.20.30.0/24 192.168.1.254
```

Change default route:

```bash
sudo route delete default 192.168.1.1
sudo route add default 192.168.1.254
```

Notes

- Both demos also print JSON payloads through the callback result channel.
- Stop a demo with `Ctrl+C`.
- These demos are intentionally simple so individual sensors can be tested in isolation.
