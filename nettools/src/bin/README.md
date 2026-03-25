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
  - Supports both IPv4 and IPv6 sockets.

- `nettools-neighbours`
  - Watches ARP and neighbour-table entries.
  - Prints events when neighbours are added, removed, or changed.
  - Supports IPv4 ARP entries and IPv6 neighbour entries when the host exposes them.

- `nettools-route-lookup`
  - Watches how the system would route specific destinations.
  - Prints events when the chosen route changes.
  - Supports both IPv4 and IPv6 targets.

- `nettools-throughput`
  - Watches interface counters and calculates byte and packet rates.
  - Prints events when interface throughput changes.

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

Run the neighbours demo:

```bash
cargo run -p nettools --bin nettools-neighbours
```

Run the route lookup demo:

```bash
cargo run -p nettools --bin nettools-route-lookup -- 8.8.8.8 2001:4860:4860::8888
```

Run the throughput demo:

```bash
cargo run -p nettools --bin nettools-throughput
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

Open an IPv6 listener with `nc`:

```bash
nc -6 -l ::1 8080
```

Open a listener with Python if `nc` is not available:

```bash
python3 -m http.server 8080
```

Inspect what your system already exposes:

```bash
ss -lntup
```

`nettools-neighbours`

- Start the binary.
- Cause ARP or neighbour discovery in another shell.
- The demo prints lines such as:

```text
neighbour added: 192.168.1.10 lladdr aa:bb:cc:dd:ee:ff dev eth0 state=0x2
neighbour changed: 192.168.1.10 lladdr aa:bb:cc:dd:ee:ff dev eth0 -> lladdr 11:22:33:44:55:66 dev wlan0
```

Examples for neighbour changes

Populate ARP or neighbour cache by probing a host:

```bash
ping -c 1 192.168.1.10
```

Populate an IPv6 neighbour entry:

```bash
ping6 -c 1 fe80::1%eth0
```

Show current neighbour entries with common tools if present:

```bash
ip neigh
arp -an
```

Delete an entry to force re-learning:

```bash
sudo ip neigh del 192.168.1.10 dev eth0
```

`nettools-route-lookup`

- Start the binary with one or more destination IPs.
- Change routes in another shell.
- The demo prints lines such as:

```text
route lookup changed: 8.8.8.8 via 10.0.0.1 dev eth0 -> via 10.0.0.254 dev eth1
route lookup changed: 2001:4860:4860::8888 via fe80::1 dev em0 -> via fe80::2 dev em1
```

Examples for route lookup changes

Linux:

```bash
sudo ip route replace default via 10.0.0.254 dev eth0
sudo ip -6 route replace default via fe80::2 dev eth0
```

BSD:

```bash
sudo route add default 10.0.0.254
sudo route add -inet6 default fe80::2
```

`nettools-throughput`

- Start the binary.
- Generate interface traffic in another shell.
- The demo prints lines such as:

```text
throughput: eth0 rx=1048576B/s tx=65536B/s rx_pps=800 tx_pps=120
```

Examples for traffic generation

Download something:

```bash
curl -L https://example.com/ -o /dev/null
```

Send ICMP traffic:

```bash
ping -c 20 1.1.1.1
ping6 -c 20 2606:4700:4700::1111
```

Notes

- Both demos also print JSON payloads through the callback result channel.
- Stop a demo with `Ctrl+C`.
- These demos are intentionally simple so individual sensors can be tested in isolation.
