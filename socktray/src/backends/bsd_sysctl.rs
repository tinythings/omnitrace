use crate::SockBackend;
use crate::backends::netstat_cmd::NetstatBackend;
use crate::events::SockKey;
use std::collections::HashSet;
use std::ffi::CString;
use std::ffi::CStr;
use std::io;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, Ordering};

const SOCKTRAY_PROTO_TCP: c_int = 1;
const SOCKTRAY_PROTO_TCP6: c_int = 2;
const SOCKTRAY_PROTO_UDP: c_int = 3;
const SOCKTRAY_PROTO_UDP6: c_int = 4;

#[repr(C)]
struct SocktrayBsdEntry {
    proto_kind: c_int,
    local: [c_char; 96],
    remote: [c_char; 96],
    state: [c_char; 32],
}

unsafe extern "C" {
    fn socktray_bsd_collect(out_entries: *mut *mut SocktrayBsdEntry, out_count: *mut usize) -> c_int;
    fn socktray_bsd_free(entries: *mut SocktrayBsdEntry);
}

pub struct BsdSysctlBackend {
    disabled: AtomicBool,
}

impl Default for BsdSysctlBackend {
    fn default() -> Self {
        Self { disabled: AtomicBool::new(false) }
    }
}

#[cfg(target_os = "netbsd")]
pub fn supported() -> bool {
    let names = [
        "net.inet.tcp.pcblist",
        "net.inet6.tcp6.pcblist",
        "net.inet.udp.pcblist",
        "net.inet6.udp6.pcblist",
    ];

    for name in names {
        let Ok(cname) = CString::new(name) else {
            continue;
        };

        // Name existing is not enough on some NetBSD kernels: pcb sysctls can be
        // intentionally blocked with ENOTSUP (use netstat/sockstat instead).
        let mut len: libc::size_t = 0;
        let rc = unsafe { libc::sysctlbyname(cname.as_ptr(), std::ptr::null_mut(), &mut len as *mut _, std::ptr::null(), 0) };
        if rc == 0 {
            return true;
        }

        let err = io::Error::last_os_error().raw_os_error();
        // Treat "buffer too small / size probe style" as potentially supported.
        if matches!(err, Some(x) if x == libc::ENOMEM || x == libc::E2BIG || x == libc::EINVAL) {
            return true;
        }
    }
    false
}

#[cfg(target_os = "freebsd")]
pub fn supported() -> bool {
    true
}

fn c_array_to_string(buf: &[c_char]) -> String {
    unsafe { CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned() }
}

fn proto_name(kind: c_int) -> Option<&'static str> {
    match kind {
        SOCKTRAY_PROTO_TCP => Some("tcp"),
        SOCKTRAY_PROTO_TCP6 => Some("tcp6"),
        SOCKTRAY_PROTO_UDP => Some("udp"),
        SOCKTRAY_PROTO_UDP6 => Some("udp6"),
        _ => None,
    }
}

#[async_trait::async_trait]
impl SockBackend for BsdSysctlBackend {
    async fn list(&self) -> io::Result<HashSet<SockKey>> {
        if self.disabled.load(Ordering::Relaxed) {
            return NetstatBackend.list().await;
        }

        let sysctl_out = {
            let mut ptr: *mut SocktrayBsdEntry = std::ptr::null_mut();
            let mut count: usize = 0;

            let rc = unsafe { socktray_bsd_collect(&mut ptr as *mut _, &mut count as *mut _) };
            if rc != 0 {
                Err(io::Error::last_os_error())
            } else if ptr.is_null() || count == 0 {
                Ok(None)
            } else {
                let mut out = HashSet::with_capacity(count);
                unsafe {
                    let slice = std::slice::from_raw_parts(ptr, count);
                    for e in slice {
                        let Some(proto) = proto_name(e.proto_kind) else {
                            continue;
                        };
                        let local = c_array_to_string(&e.local);
                        let remote = c_array_to_string(&e.remote);
                        let state_txt = c_array_to_string(&e.state);
                        let state = if state_txt.is_empty() { None } else { Some(state_txt.clone()) };

                        out.insert(SockKey {
                            proto: proto.to_string(),
                            local: local.clone(),
                            remote: remote.clone(),
                            state,
                            local_dec: Some(local),
                            remote_dec: Some(remote),
                            state_dec: if state_txt.is_empty() { None } else { Some(state_txt) },
                            remote_host: None,
                        });
                    }
                    socktray_bsd_free(ptr);
                }

                if out.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(out))
                }
            }
        };

        match sysctl_out {
            Ok(Some(out)) => Ok(out),
            Ok(None) => NetstatBackend.list().await,
            Err(err) => {
                self.disabled.store(true, Ordering::Relaxed);
                let raw = err.raw_os_error();
                let expected = matches!(
                    raw,
                    Some(x)
                        if x == libc::ENOTSUP
                            || x == libc::EOPNOTSUPP
                            || x == libc::EPERM
                            || x == libc::EACCES
                            || x == libc::EINVAL
                );
                if !expected {
                    eprintln!("socktray: bsd sysctl backend failed ({err}); disabling it and using netstat fallback");
                }
                NetstatBackend.list().await
            }
        }
    }
}
