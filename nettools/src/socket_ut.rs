use crate::socket::LiveSocketBackend;

#[test]
fn decode_addr_formats_ipv4() {
    assert_eq!(LiveSocketBackend::decode_addr("0100007F:0016", false).unwrap(), "127.0.0.1:22");
}

#[test]
fn decode_addr_formats_ipv6() {
    assert_eq!(LiveSocketBackend::decode_addr("00000000000000000000000000000001:01BB", true).unwrap(), "[::1]:443");
}

#[test]
fn decode_tcp_state_maps_listen() {
    assert_eq!(LiveSocketBackend::decode_tcp_state(Some("0A")).as_deref(), Some("LISTEN"));
}

#[test]
fn unspecified_remote_detects_ipv6_listener_marker() {
    assert!(LiveSocketBackend::is_unspecified_remote("[::]:0"));
}
