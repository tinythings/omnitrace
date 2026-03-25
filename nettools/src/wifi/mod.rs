use crate::{events, NetTools, WifiBackend};
use omnitrace_core::callbacks::CallbackHub;
use std::{collections::HashMap, io, process::Command};

pub struct LiveWifiBackend;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    impl LiveWifiBackend {
        fn parse_wireless_float(value: &str) -> f32 {
            value.trim_end_matches('.').parse::<f32>().unwrap_or(0.0)
        }

        pub(crate) fn parse_wireless_line(line: &str) -> Option<events::WifiDetails> {
            line.split_once(':').and_then(|(iface, stats)| {
                let fs = stats.split_whitespace().collect::<Vec<_>>();
                (fs.len() >= 4).then(|| events::WifiDetails {
                    iface: iface.trim().to_string(),
                    connected: fs[0] != "0000",
                    link_quality: Self::parse_wireless_float(fs[1]),
                    signal_level_dbm: Self::parse_wireless_float(fs[2]),
                    noise_level_dbm: Self::parse_wireless_float(fs[3]),
                    ssid: None,
                    bssid: None,
                })
            })
        }

        fn enrich_from_iw_command(iface: &str, wifi: &mut events::WifiDetails) {
            let iw_output = ["/usr/sbin/iw", "/usr/bin/iw"]
                .iter()
                .find_map(|path| {
                    std::path::Path::new(path)
                        .exists()
                        .then(|| Command::new(path).args(["dev", iface, "link"]).output().ok())
                        .flatten()
                });

            if let Some(out) = iw_output {
                let txt = String::from_utf8_lossy(&out.stdout);

                if txt.lines().any(|line| line.trim() == "Not connected.") {
                    wifi.connected = false;
                    wifi.ssid = None;
                    wifi.bssid = None;
                    return;
                }

                txt.lines().for_each(|line| {
                    let t = line.trim();

                    if let Some(bssid) = t.strip_prefix("Connected to ") {
                        wifi.bssid = bssid.split_whitespace().next().map(str::to_string);
                    } else if let Some(ssid) = t.strip_prefix("SSID: ") {
                        wifi.ssid = Some(ssid.to_string());
                    } else if let Some(sig) = t.strip_prefix("signal: ") {
                        wifi.signal_level_dbm = sig
                            .split_whitespace()
                            .next()
                            .and_then(|value| value.parse::<f32>().ok())
                            .unwrap_or(wifi.signal_level_dbm);
                    }
                });
            }
        }

        pub(crate) fn parse_proc_net_wireless(content: &str) -> HashMap<String, events::WifiDetails> {
            content
                .lines()
                .skip(2)
                .filter_map(Self::parse_wireless_line)
                .map(|mut wifi| {
                    let ifn = wifi.iface.clone();
                    Self::enrich_from_iw_command(&ifn, &mut wifi);
                    (ifn, wifi)
                })
                .collect()
        }
    }

    impl WifiBackend for LiveWifiBackend {
        fn list(&self) -> io::Result<HashMap<String, events::WifiDetails>> {
            std::fs::read_to_string("/proc/net/wireless").map(|content| Self::parse_proc_net_wireless(&content))
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod unsupported {
    use super::*;

    impl WifiBackend for LiveWifiBackend {
        fn list(&self) -> io::Result<HashMap<String, events::WifiDetails>> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("wifi backend is not implemented for {}", std::env::consts::OS),
            ))
        }
    }
}

impl NetTools {
    pub(crate) fn poll_wifi(&self) -> io::Result<HashMap<String, events::WifiDetails>> {
        self.wifi_backend.list()
    }

    pub(crate) async fn handle_wifi_poll(&mut self, hub: &CallbackHub<events::NetToolsEvent>) {
        let cur = match self.poll_wifi() {
            Ok(cur) => cur,
            Err(err) => {
                log::error!("nettools: failed to read wifi details: {err}");
                return;
            }
        };

        for (i, cur_w) in &cur {
            if let Some(old) = self.last_wifi.get(i) {
                if old != cur_w {
                    Self::fire(
                        hub,
                        events::NetToolsEvent::WifiChanged {
                            old: old.clone(),
                            new: cur_w.clone(),
                        },
                    )
                    .await;
                }
            } else {
                Self::fire(hub, events::NetToolsEvent::WifiAdded { wifi: cur_w.clone() }).await;
            }
        }

        for (i, old) in &self.last_wifi {
            if !cur.contains_key(i) {
                Self::fire(hub, events::NetToolsEvent::WifiRemoved { wifi: old.clone() }).await;
            }
        }

        self.last_wifi = cur;
    }
}
