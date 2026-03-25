use crate::{events, nethealth::NetHealthSample, NetTools, NetToolsConfig};

#[test]
fn nethealth_state_reports_down_when_all_probes_fail() {
    let mut s = NetTools::new(Some(NetToolsConfig::default().nethealth(true).nethealth_window(2)));
    s.nethealth_samples.push_back(NetHealthSample {
        total_probes: 2,
        successful_probes: 0,
        latency_sum_ms: 0,
    });

    assert_eq!(s.nethealth_state().unwrap().level, events::NetHealthLevel::Down);
}

#[test]
fn nethealth_state_reports_degraded_on_loss_threshold() {
    let mut s = NetTools::new(Some(NetToolsConfig::default().nethealth(true).nethealth_window(2)));
    s.nethealth_samples.push_back(NetHealthSample {
        total_probes: 4,
        successful_probes: 3,
        latency_sum_ms: 120,
    });

    let st = s.nethealth_state().unwrap();
    assert_eq!(st.level, events::NetHealthLevel::Degraded);
    assert_eq!(st.loss_pct, 25);
}

#[test]
fn trim_nethealth_samples_keeps_window_size() {
    let mut s = NetTools::new(Some(NetToolsConfig::default().nethealth(true).nethealth_window(2)));
    s.nethealth_samples.push_back(NetHealthSample {
        total_probes: 1,
        successful_probes: 1,
        latency_sum_ms: 10,
    });
    s.nethealth_samples.push_back(NetHealthSample {
        total_probes: 1,
        successful_probes: 1,
        latency_sum_ms: 20,
    });
    s.nethealth_samples.push_back(NetHealthSample {
        total_probes: 1,
        successful_probes: 1,
        latency_sum_ms: 30,
    });

    s.trim_nethealth_samples();

    assert_eq!(s.nethealth_samples.len(), 2);
    assert_eq!(s.nethealth_samples.front().unwrap().latency_sum_ms, 20);
}
