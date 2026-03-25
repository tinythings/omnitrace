use crate::neighbour::LiveNeighbourBackend;

#[test]
fn parse_proc_net_arp_extracts_ipv4_entry() {
    let ns = LiveNeighbourBackend::parse_proc_net_arp(
        "IP address       HW type     Flags       HW address            Mask     Device\n192.168.1.10 0x1 0x2 aa:bb:cc:dd:ee:ff * eth0\n",
    );
    assert_eq!(ns["192.168.1.10"].mac, "aa:bb:cc:dd:ee:ff");
    assert_eq!(ns["192.168.1.10"].iface, "eth0");
}

#[test]
fn parse_arp_line_extracts_fields() {
    let n = LiveNeighbourBackend::parse_arp_line("10.0.0.2 at 11:22:33:44:55:66 on em0").unwrap();
    assert_eq!(n.address, "10.0.0.2");
    assert_eq!(n.mac, "11:22:33:44:55:66");
    assert_eq!(n.iface, "em0");
}

#[test]
fn parse_ip_neigh_line_extracts_ipv6_entry() {
    let n = LiveNeighbourBackend::parse_ip_neigh_line("fe80::1 dev wlan0 lladdr aa:bb:cc:dd:ee:ff REACHABLE").unwrap();
    assert_eq!(n.address, "fe80::1");
    assert_eq!(n.iface, "wlan0");
    assert_eq!(n.state.as_deref(), Some("REACHABLE"));
}
