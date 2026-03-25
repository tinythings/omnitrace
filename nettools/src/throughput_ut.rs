use crate::{events, throughput::LiveThroughputBackend, NetTools};
use std::time::Duration;

#[test]
fn parse_proc_net_dev_line_extracts_counters() {
    let c = LiveThroughputBackend::parse_proc_net_dev_line("eth0: 100 2 3 4 0 0 0 0 200 5 6 7 0 0 0 0").unwrap();
    assert_eq!(c.iface, "eth0");
    assert_eq!(c.rx_bytes, 100);
    assert_eq!(c.tx_packets, 5);
    assert_eq!(c.tx_drops, 7);
}

#[test]
fn parse_netstat_ib_line_extracts_iface_and_bytes() {
    let c = LiveThroughputBackend::parse_netstat_ib_line("em0 1500 <Link#1> aa:bb:cc:dd:ee:ff 1 2 7 1000 8 2000").unwrap();
    assert_eq!(c.iface, "em0");
    assert_eq!(c.rx_bytes, 1000);
    assert_eq!(c.tx_bytes, 2000);
}

#[test]
fn throughput_sample_calculates_rates() {
    let a = events::InterfaceCounters {
        iface: "eth0".to_string(),
        rx_bytes: 1000,
        rx_packets: 10,
        rx_errors: 0,
        rx_drops: 0,
        tx_bytes: 2000,
        tx_packets: 20,
        tx_errors: 0,
        tx_drops: 0,
    };
    let b = events::InterfaceCounters {
        iface: "eth0".to_string(),
        rx_bytes: 3000,
        rx_packets: 30,
        rx_errors: 0,
        rx_drops: 0,
        tx_bytes: 5000,
        tx_packets: 50,
        tx_errors: 0,
        tx_drops: 0,
    };
    let s = NetTools::throughput_sample(&a, &b, Duration::from_secs(2)).unwrap();
    assert_eq!(s.rx_bytes_per_sec, 1000);
    assert_eq!(s.tx_packets_per_sec, 15);
}
