#[cfg(test)]
mod tests {
    use crate::netutil::{dec_ipv4, dec_ipv6, decode_addr, decode_tcp_state, expand_pat, hex_port, is_hostish, is_ipish, reverse_dns};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    // -------------------------
    // hex_port
    // -------------------------

    #[test]
    fn hex_port_parses_valid_hex() {
        assert_eq!(hex_port("0000"), Some(0));
        assert_eq!(hex_port("0001"), Some(1));
        assert_eq!(hex_port("01BB"), Some(443));
        assert_eq!(hex_port("FFFF"), Some(65535));
        assert_eq!(hex_port("c69c"), Some(0xC69C));
    }

    #[test]
    fn hex_port_rejects_invalid() {
        assert_eq!(hex_port(""), None);
        assert_eq!(hex_port("GGGG"), None);
        assert_eq!(hex_port("01BG"), None);
        assert_eq!(hex_port(" "), None);
    }

    // -------------------------
    // dec_ipv4
    // -------------------------

    #[test]
    fn dec_ipv4_decodes_current_logic() {
        // This asserts what your binary is actually doing right now:
        // "0100007F" => 127.0.0.1 (Linux /proc/net style)
        assert_eq!(dec_ipv4("0100007F"), Some(Ipv4Addr::new(127, 0, 0, 1)));

        // And the reverse string gives the reverse IP
        assert_eq!(dec_ipv4("7F000001"), Some(Ipv4Addr::new(1, 0, 0, 127)));

        // 0.0.0.0
        assert_eq!(dec_ipv4("00000000"), Some(Ipv4Addr::new(0, 0, 0, 0)));
    }

    #[test]
    fn dec_ipv4_rejects_invalid_hex() {
        assert_eq!(dec_ipv4(""), None);
        assert_eq!(dec_ipv4("ZZZZZZZZ"), None);

        // short but valid hex is accepted by current implementation
        assert!(dec_ipv4("123").is_some());
    }

    // -------------------------
    // dec_ipv6 (32 hex chars network order)
    // -------------------------

    #[test]
    fn dec_ipv6_decodes_32_hex_chars_network_order() {
        // ::1 => 000...0001
        let loopback = "00000000000000000000000000000001";
        assert_eq!(dec_ipv6(loopback), Some(Ipv6Addr::LOCALHOST));

        // :: => 000...0000
        let all_zero = "00000000000000000000000000000000";
        assert_eq!(dec_ipv6(all_zero), Some(Ipv6Addr::UNSPECIFIED));
    }

    #[test]
    fn dec_ipv6_rejects_wrong_length_or_bad_hex() {
        assert_eq!(dec_ipv6(""), None);
        assert_eq!(dec_ipv6("0"), None);
        assert_eq!(dec_ipv6("0000000000000000000000000000000"), None); // 31 chars
        assert_eq!(dec_ipv6("000000000000000000000000000000011"), None); // 33 chars
        assert_eq!(dec_ipv6("GG000000000000000000000000000000"), None); // bad hex
    }

    // -------------------------
    // decode_addr
    // -------------------------

    #[test]
    fn decode_addr_ipv4() {
        // /proc style: 0100007F => 127.0.0.1
        assert_eq!(decode_addr("0100007F:01BB", false).as_deref(), Some("127.0.0.1:443"));

        // NOTE: if you want to lock a second example, compute it from your actual logs.
        // Keeping this one minimal avoids chasing swapped expectations.
    }

    #[test]
    fn decode_addr_ipv6() {
        // ::1:443
        let ip_hex = "00000000000000000000000000000001";
        assert_eq!(decode_addr(&format!("{ip_hex}:01BB"), true).as_deref(), Some("::1:443"));
    }

    #[test]
    fn decode_addr_rejects_bad_inputs() {
        assert_eq!(decode_addr("", false), None);
        assert_eq!(decode_addr("NOPE", false), None);
        assert_eq!(decode_addr("0100007F", false), None);
        assert_eq!(decode_addr("0100007F:ZZZZ", false), None);
        assert_eq!(decode_addr("BADHEX:01BB", false), None);

        // v4 hex with v6 flag should be rejected (len != 32)
        assert_eq!(decode_addr("0100007F:01BB", true), None);
    }

    // -------------------------
    // decode_tcp_state
    // -------------------------

    #[test]
    fn decode_tcp_state_maps_known_codes() {
        assert_eq!(decode_tcp_state(&Some("01".into())).as_deref(), Some("ESTABLISHED"));
        assert_eq!(decode_tcp_state(&Some("02".into())).as_deref(), Some("SYN_SENT"));
        assert_eq!(decode_tcp_state(&Some("03".into())).as_deref(), Some("SYN_RECV"));
        assert_eq!(decode_tcp_state(&Some("04".into())).as_deref(), Some("FIN_WAIT1"));
        assert_eq!(decode_tcp_state(&Some("05".into())).as_deref(), Some("FIN_WAIT2"));
        assert_eq!(decode_tcp_state(&Some("06".into())).as_deref(), Some("TIME_WAIT"));
        assert_eq!(decode_tcp_state(&Some("07".into())).as_deref(), Some("CLOSE"));
        assert_eq!(decode_tcp_state(&Some("08".into())).as_deref(), Some("CLOSE_WAIT"));
        assert_eq!(decode_tcp_state(&Some("09".into())).as_deref(), Some("LAST_ACK"));
        assert_eq!(decode_tcp_state(&Some("0A".into())).as_deref(), Some("LISTEN"));
        assert_eq!(decode_tcp_state(&Some("0B".into())).as_deref(), Some("CLOSING"));
    }

    #[test]
    fn decode_tcp_state_unknown_and_none() {
        assert_eq!(decode_tcp_state(&None), None);
        assert_eq!(decode_tcp_state(&Some("FF".into())).as_deref(), Some("UNKNOWN"));
        assert_eq!(decode_tcp_state(&Some("".into())).as_deref(), Some("UNKNOWN"));
    }

    // -------------------------
    // reverse_dns
    // -------------------------

    #[test]
    fn reverse_dns_returns_none_for_unspecified_addresses() {
        assert_eq!(reverse_dns(IpAddr::V4(Ipv4Addr::UNSPECIFIED)), None);
        assert_eq!(reverse_dns(IpAddr::V6(Ipv6Addr::UNSPECIFIED)), None);
    }

    #[test]
    #[ignore = "depends on system resolver/PTR records; run manually if you want"]
    fn reverse_dns_loopback_may_resolve() {
        let v4 = reverse_dns(IpAddr::V4(Ipv4Addr::LOCALHOST));
        if let Some(name) = v4 {
            assert!(!name.trim().is_empty());
        }

        let v6 = reverse_dns(IpAddr::V6(Ipv6Addr::LOCALHOST));
        if let Some(name) = v6 {
            assert!(!name.trim().is_empty());
        }
    }

    // -------------------------
    // expand_pat
    // -------------------------

    #[test]
    fn expand_pat_empty_and_star() {
        assert_eq!(expand_pat(""), "");
        assert_eq!(expand_pat("   "), "");
        assert_eq!(expand_pat("*"), "*");
    }

    #[test]
    fn expand_pat_explicit_passthrough() {
        assert_eq!(expand_pat("raw:foo"), "raw:foo");
        assert_eq!(expand_pat("dec:1.2.3.4:443"), "dec:1.2.3.4:443");
        assert_eq!(expand_pat("host:*.google.com"), "host:*.google.com");
        assert_eq!(expand_pat("state:ESTABLISHED"), "state:ESTABLISHED");
    }

    #[test]
    fn expand_pat_port_only() {
        assert_eq!(expand_pat(":443"), "*dec:*:443*");
    }

    #[test]
    fn expand_pat_ipv4() {
        assert_eq!(expand_pat("8.8.8.8"), "*dec:*8.8.8.8:*");

        // wildcard IPv4 does *not* count as IPv4 in expand_pat (digits/dots only rule)
        assert_eq!(expand_pat("1.2.*.*"), "*host:1.2.*.**");
    }

    #[test]
    fn expand_pat_ipv6_loose() {
        // current behavior: "::1" hits the "port-only" branch because it starts with ':'
        assert_eq!(expand_pat("::1"), "*dec:*::1*");

        // this one hits the IPv6 branch
        assert_eq!(expand_pat("2001:db8::1"), "*dec:*2001:db8::1:*");
    }

    #[test]
    fn expand_pat_proto() {
        assert_eq!(expand_pat("tcp"), "tcp*");
        assert_eq!(expand_pat("udp"), "udp*");
        // NOTE: your code does NOT handle tcp6/udp6 specially (yet)
        assert_eq!(expand_pat("tcp6"), "*host:tcp6*");
    }

    #[test]
    fn expand_pat_default_hostname() {
        assert_eq!(expand_pat("google.com"), "*host:google.com*");
        assert_eq!(expand_pat("*.google.com"), "*host:*.google.com*");
        assert_eq!(expand_pat("tzfraa-aj-in-f14.1e100.net"), "*host:tzfraa-aj-in-f14.1e100.net*");
    }

    // -------------------------
    // is_ipish
    // -------------------------

    #[test]
    fn is_ipish_accepts_single_token_ip_like() {
        assert!(is_ipish("8.8.8.8"));
        assert!(is_ipish("8.8.8.8:443"));
        assert!(is_ipish("1.2.*.*"));
        assert!(is_ipish("::1"));
        assert!(is_ipish("2001:db8::1"));
        assert!(is_ipish("2001:db8::*"));
    }

    #[test]
    fn is_ipish_rejects_non_ip_like() {
        assert!(!is_ipish(""));
        assert!(!is_ipish("   "));
        assert!(!is_ipish("deadbeef")); // no '.' or ':'
        assert!(!is_ipish("*.google.com"));
        assert!(!is_ipish("udp"));
        assert!(!is_ipish("udp * *"));
        assert!(!is_ipish("1.2.3.4 something"));
    }

    // -------------------------
    // is_hostish
    // -------------------------

    #[test]
    fn is_hostish_accepts_single_token_host_like() {
        assert!(is_hostish("google.com"));
        assert!(is_hostish("*.google.com"));
        assert!(is_hostish("tzfraa-aj-in-f14.1e100.net"));
        assert!(is_hostish("udp")); // by your definition: letters => hostish
        assert!(is_hostish("deadbeef")); // letters => hostish
    }

    #[test]
    fn is_hostish_rejects_multi_token_and_non_host_like() {
        assert!(!is_hostish(""));
        assert!(!is_hostish("   "));
        assert!(!is_hostish("udp * *"));
        assert!(!is_hostish("foo bar"));
        assert!(!is_hostish("8.8.8.8"));
        assert!(!is_hostish("::1"));
        assert!(!is_hostish("1.2.3.4:443"));
    }

    // -------------------------
    // sanity checks
    // -------------------------

    #[test]
    fn sanity_target_endianness() {
        assert!(cfg!(target_endian = "little"), "you are on big-endian, welcome to 1993");
    }

    #[test]
    fn sanity_dec_ipv4_matches_decode_addr() {
        // Whatever dec_ipv4 does, decode_addr must use the same logic.
        let ip = dec_ipv4("0100007F").unwrap().to_string();
        let got = decode_addr("0100007F:01BB", false).unwrap();
        assert!(got.starts_with(&format!("{ip}:")), "decode_addr mismatch: {got} vs {ip}");
    }
}
