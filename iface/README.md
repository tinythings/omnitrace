# iface

Cross-platform interface/link/address sensor.

Backends:
- Linux / Android: `rtnetlink`
- NetBSD / FreeBSD: `PF_ROUTE` trigger + interface snapshot diff

Events:
- `IfaceAdded`
- `IfaceRemoved`
- `LinkUp`
- `LinkDown`
- `AddrAdded`
- `AddrRemoved`

## Demo

Run:

```bash
cargo run -p iface
```

Trigger events in another terminal.

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
