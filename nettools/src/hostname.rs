use crate::{events, HostnameBackend, NetTools};
use omnitrace_core::callbacks::CallbackHub;
use std::io;

pub struct LiveHostnameBackend;

impl LiveHostnameBackend {
    fn read_hostname() -> io::Result<String> {
        let mut buf = [0u8; 256];
        if unsafe { libc::gethostname(buf.as_mut_ptr().cast::<libc::c_char>(), buf.len()) } != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(String::from_utf8_lossy(&buf[..buf.iter().position(|b| *b == 0).unwrap_or(buf.len())])
            .trim()
            .to_string())
    }
}

impl HostnameBackend for LiveHostnameBackend {
    fn current(&self) -> io::Result<String> {
        Self::read_hostname()
    }
}

impl NetTools {
    pub(crate) fn poll_hostname(&self) -> io::Result<String> {
        self.hostname_backend.current()
    }

    pub(crate) fn store_hostname(&mut self, hostname: String) {
        self.last_hostname = Some(hostname);
    }

    pub(crate) async fn handle_hostname_poll(&mut self, hub: &CallbackHub<events::NetToolsEvent>) {
        let cur = match self.poll_hostname() {
            Ok(cur) => cur,
            Err(err) => {
                log::error!("nettools: failed to read hostname: {err}");
                return;
            }
        };

        if let Some(old) = self.last_hostname.as_ref()
            && old != &cur
        {
            Self::fire(
                hub,
                events::NetToolsEvent::HostnameChanged {
                    old: old.clone(),
                    new: cur.clone(),
                },
            )
            .await;
        }

        self.store_hostname(cur);
    }
}
