use crate::{events, route::LiveRouteBackend, NetTools};

fn r(f: events::RouteFamily, d: &str, g: &str, i: &str) -> events::RouteEntry {
    events::RouteEntry {
        family: f,
        destination: d.to_string(),
        gateway: g.to_string(),
        iface: i.to_string(),
    }
}

#[test]
fn parse_line_detects_ipv6_family_from_fields() {
    let r = LiveRouteBackend::parse_line("2001:db8::/64 fe80::1 UGS em0", &events::RouteFamily::Unknown).unwrap();
    assert_eq!(r.family, events::RouteFamily::Inet6);
    assert_eq!(r.iface, "em0");
}

#[test]
fn parse_routes_tracks_family_sections() {
    let rs = LiveRouteBackend::parse_routes(
        "Internet:\ndefault 10.0.0.1 UGS em0\nInternet6:\n::/0 fe80::1 UGS em1\n",
    );
    assert_eq!(rs.len(), 2);
    assert_eq!(rs[0].family, events::RouteFamily::Inet);
    assert_eq!(rs[1].family, events::RouteFamily::Inet6);
}

#[test]
fn route_matches_target_prefers_ipv4_prefix() {
    assert!(NetTools::route_matches_target(&r(events::RouteFamily::Inet, "10.20.30.0/24", "10.0.0.1", "em0"), "10.20.30.40".parse().unwrap()));
    assert!(!NetTools::route_matches_target(&r(events::RouteFamily::Inet, "10.20.31.0/24", "10.0.0.1", "em0"), "10.20.30.40".parse().unwrap()));
}

#[test]
fn route_lookup_uses_longest_prefix_match() {
    let rs = NetTools::route_map(vec![
        r(events::RouteFamily::Inet, "default", "10.0.0.1", "em0"),
        r(events::RouteFamily::Inet, "10.20.30.0/24", "10.0.0.254", "em1"),
    ]);

    let lu = NetTools::route_lookup(&rs, "10.20.30.40").unwrap();
    assert_eq!(lu.route.gateway, "10.0.0.254");
    assert_eq!(lu.route.iface, "em1");
}

#[test]
fn default_route_recognises_ipv6_default() {
    let rs = NetTools::route_map(vec![r(events::RouteFamily::Inet6, "::/0", "fe80::1", "em0")]);
    assert_eq!(NetTools::default_route(&rs).unwrap().gateway, "fe80::1");
}
