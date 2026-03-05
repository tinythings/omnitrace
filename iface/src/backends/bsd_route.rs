use crate::IfaceBackend;
use crate::events::IfaceEvent;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::CStr;
use std::io;
use std::mem;
use std::os::fd::RawFd;
use std::time::Duration;

#[derive(Clone, Default)]
struct IfaceState {
    ifname: String,
    up: bool,
    addrs: HashSet<String>,
}

type Snapshot = HashMap<u32, IfaceState>;

pub struct BsdRouteBackend {
    fd: RawFd,
    last: Snapshot,
    queue: VecDeque<IfaceEvent>,
}

impl BsdRouteBackend {
    pub fn new() -> io::Result<Self> {
        let fd = unsafe { libc::socket(libc::PF_ROUTE, libc::SOCK_RAW, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let last = Self::collect_snapshot()?;
        Ok(Self { fd, last, queue: VecDeque::new() })
    }

    fn sockaddr_key(sa: *const libc::sockaddr) -> Option<String> {
        if sa.is_null() {
            return None;
        }
        let fam = unsafe { (*sa).sa_family as i32 };
        if fam != libc::AF_INET && fam != libc::AF_INET6 {
            return None;
        }

        let mut host = [0i8; libc::NI_MAXHOST as usize];
        let salen = match fam {
            libc::AF_INET => mem::size_of::<libc::sockaddr_in>(),
            libc::AF_INET6 => mem::size_of::<libc::sockaddr_in6>(),
            _ => return None,
        } as libc::socklen_t;

        let rc = unsafe {
            libc::getnameinfo(
                sa,
                salen,
                host.as_mut_ptr(),
                host.len() as libc::socklen_t,
                std::ptr::null_mut(),
                0,
                libc::NI_NUMERICHOST,
            )
        };
        if rc != 0 {
            return None;
        }
        Some(unsafe { CStr::from_ptr(host.as_ptr()).to_string_lossy().into_owned() })
    }

    fn collect_snapshot() -> io::Result<Snapshot> {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        let rc = unsafe { libc::getifaddrs(&mut head as *mut *mut libc::ifaddrs) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        let mut map: Snapshot = HashMap::new();
        let mut cur = head;
        while !cur.is_null() {
            let ifa = unsafe { &*cur };
            if !ifa.ifa_name.is_null() {
                let ifname = unsafe { CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned() };
                let ifindex = unsafe { libc::if_nametoindex(ifa.ifa_name) };
                if ifindex != 0 {
                    let e = map.entry(ifindex).or_insert_with(|| IfaceState {
                        ifname: ifname.clone(),
                        up: false,
                        addrs: HashSet::new(),
                    });
                    e.ifname = ifname;
                    e.up = (ifa.ifa_flags as i32 & libc::IFF_UP) != 0;
                    if let Some(addr) = Self::sockaddr_key(ifa.ifa_addr) {
                        e.addrs.insert(addr);
                    }
                }
            }
            cur = ifa.ifa_next;
        }

        unsafe {
            libc::freeifaddrs(head);
        }
        Ok(map)
    }

    fn diff_into_queue(old: &Snapshot, new: &Snapshot, q: &mut VecDeque<IfaceEvent>) {
        for (idx, ns) in new {
            match old.get(idx) {
                None => {
                    q.push_back(IfaceEvent::IfaceAdded { ifindex: *idx, ifname: ns.ifname.clone() });
                    if ns.up {
                        q.push_back(IfaceEvent::LinkUp { ifindex: *idx, ifname: ns.ifname.clone() });
                    } else {
                        q.push_back(IfaceEvent::LinkDown { ifindex: *idx, ifname: ns.ifname.clone() });
                    }
                    for _ in &ns.addrs {
                        q.push_back(IfaceEvent::AddrAdded { ifindex: *idx, ifname: ns.ifname.clone() });
                    }
                }
                Some(os) => {
                    if os.up != ns.up {
                        if ns.up {
                            q.push_back(IfaceEvent::LinkUp { ifindex: *idx, ifname: ns.ifname.clone() });
                        } else {
                            q.push_back(IfaceEvent::LinkDown { ifindex: *idx, ifname: ns.ifname.clone() });
                        }
                    }
                    for _ in ns.addrs.difference(&os.addrs) {
                        q.push_back(IfaceEvent::AddrAdded { ifindex: *idx, ifname: ns.ifname.clone() });
                    }
                    for _ in os.addrs.difference(&ns.addrs) {
                        q.push_back(IfaceEvent::AddrRemoved { ifindex: *idx, ifname: ns.ifname.clone() });
                    }
                }
            }
        }

        for (idx, os) in old {
            if !new.contains_key(idx) {
                q.push_back(IfaceEvent::IfaceRemoved { ifindex: *idx, ifname: os.ifname.clone() });
            }
        }
    }

    fn recv_once(&mut self, timeout: Duration) -> io::Result<()> {
        let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let prc = unsafe { libc::poll(&mut pfd as *mut libc::pollfd, 1, timeout_ms) };
        if prc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(());
            }
            return Err(err);
        }
        if prc == 0 || (pfd.revents & libc::POLLIN) == 0 {
            return Ok(());
        }

        // Drain one route message; details are diffed via snapshot.
        let mut buf = [0u8; 4096];
        let _ = unsafe { libc::recv(self.fd, buf.as_mut_ptr().cast::<libc::c_void>(), buf.len(), 0) };

        let now = Self::collect_snapshot()?;
        Self::diff_into_queue(&self.last, &now, &mut self.queue);
        self.last = now;
        Ok(())
    }
}

impl Drop for BsdRouteBackend {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

#[async_trait::async_trait]
impl IfaceBackend for BsdRouteBackend {
    async fn next_event(&mut self, timeout: Duration) -> io::Result<Option<IfaceEvent>> {
        if let Some(ev) = self.queue.pop_front() {
            return Ok(Some(ev));
        }
        tokio::task::block_in_place(|| self.recv_once(timeout))?;
        Ok(self.queue.pop_front())
    }
}
