# socktray

`socktray` is a socket activity sensor for Omnitrace.

It emits:
- `Opened`
- `Closed`

for TCP/UDP sockets by diffing periodic snapshots.

Backend strategy:
- Linux / Android: `/proc/net/{tcp,tcp6,udp,udp6}`
- Others: `netstat -an` parser fallback
