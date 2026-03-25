#[cfg(target_os = "linux")]
use crate::wifi::LiveWifiBackend;

#[cfg(target_os = "linux")]
#[test]
fn parse_wireless_line_extracts_quality_signal_and_noise() {
    let w = LiveWifiBackend::parse_wireless_line("wlp0s20f3: 0001   42.  -61.  -95.        0      0      0      0      0        0").unwrap();
    assert!(w.connected);
    assert_eq!(w.iface, "wlp0s20f3");
    assert_eq!(w.link_quality, 42.0);
    assert_eq!(w.signal_level_dbm, -61.0);
    assert_eq!(w.noise_level_dbm, -95.0);
}

#[cfg(target_os = "linux")]
#[test]
fn parse_proc_net_wireless_skips_headers() {
    let ws = LiveWifiBackend::parse_proc_net_wireless(
        "Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE\n face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22\nwlan0: 0000   30.  -70.  -90.        0      0      0      0      0        0\n",
    );
    assert_eq!(ws.len(), 1);
    assert_eq!(ws["wlan0"].signal_level_dbm, -70.0);
}
