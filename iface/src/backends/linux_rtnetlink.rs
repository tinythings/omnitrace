use crate::IfaceBackend;
use crate::events::IfaceEvent;
use std::collections::{HashMap, VecDeque};
use std::ffi::CStr;
use std::io;
use std::mem;
use std::os::fd::RawFd;
use std::time::Duration;

#[repr(C)]
struct IfInfoMsg {
    ifi_family: u8,
    ifi_pad: u8,
    ifi_type: u16,
    ifi_index: i32,
    ifi_flags: u32,
    ifi_change: u32,
}

#[repr(C)]
struct IfAddrMsg {
    ifa_family: u8,
    ifa_prefixlen: u8,
    ifa_flags: u8,
    ifa_scope: u8,
    ifa_index: u32,
}

#[repr(C)]
struct RtAttr {
    rta_len: u16,
    rta_type: u16,
}

const IFLA_IFNAME_U16: u16 = 3;

pub struct LinuxRtNetlinkBackend {
    fd: RawFd,
    queue: VecDeque<IfaceEvent>,
    known_up: HashMap<u32, bool>,
}

impl LinuxRtNetlinkBackend {
    pub fn new() -> io::Result<Self> {
        let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_RAW, libc::NETLINK_ROUTE) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let mut addr: libc::sockaddr_nl = unsafe { mem::zeroed() };
        addr.nl_family = libc::AF_NETLINK as u16;
        addr.nl_pid = 0;
        addr.nl_groups = (libc::RTMGRP_LINK | libc::RTMGRP_IPV4_IFADDR | libc::RTMGRP_IPV6_IFADDR) as u32;

        let rc = unsafe {
            libc::bind(
                fd,
                (&addr as *const libc::sockaddr_nl).cast::<libc::sockaddr>(),
                mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            let err = io::Error::last_os_error();
            unsafe {
                libc::close(fd);
            }
            return Err(err);
        }

        Ok(Self { fd, queue: VecDeque::new(), known_up: HashMap::new() })
    }

    fn nlmsg_align(v: usize) -> usize {
        (v + 3) & !3
    }

    fn rta_align(v: usize) -> usize {
        (v + 3) & !3
    }

    fn ifname_from_index(ifindex: u32) -> String {
        let mut buf = [0i8; libc::IF_NAMESIZE];
        let p = unsafe { libc::if_indextoname(ifindex, buf.as_mut_ptr()) };
        if p.is_null() {
            format!("if#{ifindex}")
        } else {
            unsafe { CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned() }
        }
    }

    fn parse_ifla_ifname(attrs: &[u8]) -> Option<String> {
        let mut off = 0usize;
        while off + mem::size_of::<RtAttr>() <= attrs.len() {
            let rta = unsafe { &*(attrs[off..].as_ptr() as *const RtAttr) };
            let len = rta.rta_len as usize;
            if len < mem::size_of::<RtAttr>() || off + len > attrs.len() {
                break;
            }

            if rta.rta_type == IFLA_IFNAME_U16 {
                let data = &attrs[off + mem::size_of::<RtAttr>()..off + len];
                let nul = data.iter().position(|b| *b == 0).unwrap_or(data.len());
                let s = String::from_utf8_lossy(&data[..nul]).to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }

            off += Self::rta_align(len);
        }
        None
    }

    fn handle_link(&mut self, msg_type: u16, payload: &[u8]) {
        if payload.len() < mem::size_of::<IfInfoMsg>() {
            return;
        }

        let ifi = unsafe { &*(payload.as_ptr() as *const IfInfoMsg) };
        let ifindex = ifi.ifi_index as u32;
        let attrs = &payload[mem::size_of::<IfInfoMsg>()..];

        let ifname = Self::parse_ifla_ifname(attrs).unwrap_or_else(|| Self::ifname_from_index(ifindex));
        let is_up = (ifi.ifi_flags as i32 & libc::IFF_UP) != 0;

        match msg_type {
            libc::RTM_DELLINK => {
                self.known_up.remove(&ifindex);
                self.queue.push_back(IfaceEvent::IfaceRemoved { ifindex, ifname });
            }
            libc::RTM_NEWLINK => {
                let prev = self.known_up.get(&ifindex).copied();
                if prev.is_none() {
                    self.queue.push_back(IfaceEvent::IfaceAdded { ifindex, ifname: ifname.clone() });
                }
                if prev != Some(is_up) {
                    if is_up {
                        self.queue.push_back(IfaceEvent::LinkUp { ifindex, ifname: ifname.clone() });
                    } else {
                        self.queue.push_back(IfaceEvent::LinkDown { ifindex, ifname: ifname.clone() });
                    }
                }
                self.known_up.insert(ifindex, is_up);
            }
            _ => {}
        }
    }

    fn handle_addr(&mut self, msg_type: u16, payload: &[u8]) {
        if payload.len() < mem::size_of::<IfAddrMsg>() {
            return;
        }
        let ifa = unsafe { &*(payload.as_ptr() as *const IfAddrMsg) };
        let ifindex = ifa.ifa_index;
        let ifname = Self::ifname_from_index(ifindex);

        match msg_type {
            libc::RTM_NEWADDR => self.queue.push_back(IfaceEvent::AddrAdded { ifindex, ifname }),
            libc::RTM_DELADDR => self.queue.push_back(IfaceEvent::AddrRemoved { ifindex, ifname }),
            _ => {}
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

        let mut buf = [0u8; 16384];
        let n = unsafe { libc::recv(self.fd, buf.as_mut_ptr().cast::<libc::c_void>(), buf.len(), 0) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock || err.kind() == io::ErrorKind::Interrupted {
                return Ok(());
            }
            return Err(err);
        }
        let n = n as usize;
        if n == 0 {
            return Ok(());
        }

        let hsz = mem::size_of::<libc::nlmsghdr>();
        let mut off = 0usize;
        while off + hsz <= n {
            let hdr = unsafe { &*(buf[off..].as_ptr() as *const libc::nlmsghdr) };
            let msg_len = hdr.nlmsg_len as usize;
            if msg_len < hsz || off + msg_len > n {
                break;
            }

            let payload = &buf[off + hsz..off + msg_len];
            match hdr.nlmsg_type {
                libc::RTM_NEWLINK | libc::RTM_DELLINK => self.handle_link(hdr.nlmsg_type, payload),
                libc::RTM_NEWADDR | libc::RTM_DELADDR => self.handle_addr(hdr.nlmsg_type, payload),
                x if x == libc::NLMSG_ERROR as u16 || x == libc::NLMSG_DONE as u16 => {}
                _ => {}
            }

            off += Self::nlmsg_align(msg_len);
        }

        Ok(())
    }
}

impl Drop for LinuxRtNetlinkBackend {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

#[async_trait::async_trait]
impl IfaceBackend for LinuxRtNetlinkBackend {
    async fn next_event(&mut self, timeout: Duration) -> io::Result<Option<IfaceEvent>> {
        if let Some(ev) = self.queue.pop_front() {
            return Ok(Some(ev));
        }

        tokio::task::block_in_place(|| self.recv_once(timeout))?;
        Ok(self.queue.pop_front())
    }
}
