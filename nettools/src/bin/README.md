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

- `nettools-default-route`
  - Watches the default route specifically.
  - Prints events when the default route is added, removed, or changed.

- `nettools-nethealth`
  - Runs active probes against configured targets.
  - Prints events when connectivity becomes degraded or goes down.

- `nettools-sockets`
  - Watches live sockets, listeners, and connections.
  - Prints events when sockets appear or disappear.

How to run

Run the hostname demo:

```bash
cargo run -p nettools --bin nettools-hostchange
```

Run the routes demo:

```bash
cargo run -p nettools --bin nettools-routes
```

Run the default route demo:

```bash
cargo run -p nettools --bin nettools-default-route
```

Run the network health demo:

```bash
cargo run -p nettools --bin nettools-nethealth
```

Run the sockets demo:

```bash
cargo run -p nettools --bin nettools-sockets
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

`nettools-default-route`

- Start the binary.
- Change only the default route in another shell.
- The demo prints lines such as:

```text
default route added: default via 192.168.1.1 dev em0
default route removed: default via 192.168.1.1 dev em0
default route changed: default via 192.168.1.1 dev em0 -> via 192.168.1.254 dev em1
```

Examples for default route changes

Linux:

Add default route:

```bash
sudo ip route add default via 192.168.1.1 dev eth0
```

Delete default route:

```bash
sudo ip route del default
```

Change default route:

```bash
sudo ip route replace default via 192.168.1.254 dev eth0
```

FreeBSD / NetBSD / OpenBSD:

Add default route:

```bash
sudo route add default 192.168.1.1
```

Delete default route:

```bash
sudo route delete default 192.168.1.1
```

Change default route:

```bash
sudo route delete default 192.168.1.1
sudo route add default 192.168.1.254
```

`nettools-nethealth`

- Start the binary.
- Interrupt connectivity, add latency, or drop packets in another shell.
- The demo prints lines such as:

```text
nethealth changed: Healthy avg=Some(42)ms loss=0 -> Degraded avg=Some(780)ms loss=0
nethealth changed: Degraded avg=Some(780)ms loss=0 -> Down avg=None loss=100
```

Examples for network hiccups

Linux:

Simulate packet loss:

```bash
sudo tc qdisc add dev eth0 root netem loss 40%
sudo tc qdisc del dev eth0 root
```

Simulate latency spike:

```bash
sudo tc qdisc add dev eth0 root netem delay 800ms
sudo tc qdisc del dev eth0 root
```

Simulate link outage:

```bash
sudo ip link set eth0 down
sudo ip link set eth0 up
```

`nettools-sockets`

- Start the binary.
- Open or close listeners and client connections in another shell.
- The demo prints lines such as:

```text
socket added: tcp listener 0.0.0.0:8080 -> 0.0.0.0:0 state=LISTEN
socket removed: tcp connection 10.0.0.5:54544 -> 10.0.0.10:443 state=ESTABLISHED
```

Examples for socket and listener changes

Open a TCP listener with `nc`:

```bash
nc -l 127.0.0.1 8080
```

Open a client connection with `nc`:

```bash
nc 127.0.0.1 8080
```

Open a listener with Python if `nc` is not available:

```bash
python3 -m http.server 8080
```

Inspect what your system already exposes:

```bash
ss -lntup
```

Notes

- Both demos also print JSON payloads through the callback result channel.
- Stop a demo with `Ctrl+C`.
- These demos are intentionally simple so individual sensors can be tested in isolation.
