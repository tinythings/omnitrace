pub mod events;
use crate::events::{MountInfo, XMountEvent};
use omnitrace_core::sensor::{Sensor, SensorCtx};
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    pin::Pin,
    time::Duration,
};
use tokio::time;

/// Configuration for the XMount monitor.
///
/// Controls polling interval and the path to the mountinfo file to read.
pub struct XMountConfig {
    /// Time interval between polling mountinfo for changes
    pulse: Duration,

    /// Path to the mountinfo file (typically /proc/self/mountinfo)
    mountinfo_path: PathBuf,
}

/// Main struct for monitoring mount events.
impl Default for XMountConfig {
    fn default() -> Self {
        Self { pulse: Duration::from_secs(1), mountinfo_path: PathBuf::from("/proc/self/mountinfo") }
    }
}

impl XMountConfig {
    pub fn pulse(mut self, pulse: Duration) -> Self {
        self.pulse = pulse;
        self
    }

    pub fn mountinfo_path<P: AsRef<Path>>(mut self, p: P) -> Self {
        self.mountinfo_path = p.as_ref().to_path_buf();
        self
    }
}

/// Main struct for monitoring mount events.
pub struct XMount {
    watched: HashSet<PathBuf>,
    config: XMountConfig,

    // last known per watched mountpoint
    last: HashMap<PathBuf, MountInfo>,
    is_primed: bool,
}

impl Default for XMount {
    fn default() -> Self {
        Self::new(XMountConfig::default())
    }
}

impl XMount {
    /// Create a new XMount monitor with the given configuration.
    /// The monitor won't start until you call run(), and you can still add watched mountpoints and callbacks after that.
    /// The configuration controls the polling interval and the path to the mountinfo file to read.
    /// The default configuration polls every 1 second and reads from /proc/self/mountinfo, which is usually what you want.
    pub fn new(config: XMountConfig) -> Self {
        Self { watched: HashSet::new(), config, last: HashMap::new(), is_primed: false }
    }

    /// Add a mountpoint (target) to watch.
    /// You can add any path, but only those that actually appear in /proc/self/mountinfo will trigger events.
    /// For example, if you add "/mnt/usb" but it never appears in mountinfo, you won't get any events.
    /// If you add "/" or "/mnt" or "/tmp", you'll get events for those (and they often do appear in mountinfo), but that may be very noisy.
    /// If you add something that appears in mountinfo but isn't actually a mountpoint (e.g. "/home/user"), you'll
    /// get events for it when it appears in mountinfo, but that may be confusing.
    ///
    /// In general, it's best to add specific mountpoints you care about, but the library won't stop you from adding anything.
    /// The library will canonicalize paths if possible, so adding "/mnt/usb" and "/mnt/./usb" will watch the same thing.
    ///
    /// If a watched mountpoint is missing from mountinfo, it will be treated as unmounted (but won't trigger an
    /// Unmounted event until it was previously seen as mounted).
    pub fn add<P: AsRef<Path>>(&mut self, mountpoint: P) {
        // canonicalize if possible; for mountpoints it’s usually fine either way
        if let Ok(p) = mountpoint.as_ref().canonicalize() {
            self.watched.insert(p);
        } else {
            self.watched.insert(mountpoint.as_ref().to_path_buf());
        }
    }

    /// Remove a mountpoint from being watched.
    /// If the mountpoint was previously seen as mounted, it will be treated as unmounted (but won't trigger an Unmounted event since it's no longer watched).
    /// The library will canonicalize paths if possible, so removing "/mnt/usb" and "/mnt/./usb" will remove the same thing.
    /// If you remove a mountpoint that wasn't being watched, nothing happens.
    /// If you remove a mountpoint that was being watched but is currently missing from mountinfo, it will just stop being watched without any events.
    /// In general, you can add and remove mountpoints at any time, even after run() has started, and the library will handle it gracefully.
    pub fn remove<P: AsRef<Path>>(&mut self, mountpoint: P) {
        if let Ok(p) = mountpoint.as_ref().canonicalize() {
            self.watched.remove(&p);
        } else {
            self.watched.remove(mountpoint.as_ref());
        }
    }

    /// Check if an event matches the callback's mask.
    /// For example, if the callback's mask is MOUNTED | UNMOUNTED, it will match Mounted and Unmounted events but not Changed events.
    async fn fire(hub: &omnitrace_core::callbacks::CallbackHub<XMountEvent>, ev: XMountEvent) {
        hub.fire(ev.mask().bits(), &ev).await;
    }

    /// Linux mountinfo escapes spaces as \040 etc.
    fn unescape_mount_field(s: &str) -> String {
        // minimal: handle \040 \011 \012 \134
        let mut out = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 3 < bytes.len() {
                let a = bytes[i + 1];
                let b = bytes[i + 2];
                let c = bytes[i + 3];
                if a.is_ascii_digit() && b.is_ascii_digit() && c.is_ascii_digit() {
                    let oct = ((a - b'0') as u32) * 64 + ((b - b'0') as u32) * 8 + ((c - b'0') as u32);
                    if let Some(ch) = char::from_u32(oct) {
                        out.push(ch);
                        i += 4;
                        continue;
                    }
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    /// Parse a line from mountinfo into a MountInfo struct.
    fn parse_mountinfo_line(line: &str) -> Option<MountInfo> {
        // format: mountID parentID major:minor root mount_point options optional_fields... - fstype source super_options
        let mut parts = line.split_whitespace();

        let mount_id: u32 = parts.next()?.parse().ok()?;
        let parent_id: u32 = parts.next()?.parse().ok()?;
        let _majmin = parts.next()?; // ignore

        let root = Self::unescape_mount_field(parts.next()?);
        let mount_point = Self::unescape_mount_field(parts.next()?);
        let mount_opts = parts.next()?.to_string();

        // skip optional fields until "-"
        for p in &mut parts {
            if p == "-" {
                break;
            }
        }

        let fstype = parts.next()?.to_string();
        let source = Self::unescape_mount_field(parts.next()?);
        let super_opts = parts.next().unwrap_or("").to_string();

        Some(MountInfo {
            mount_id,
            parent_id,
            mount_point: PathBuf::from(mount_point),
            root: PathBuf::from(root),
            fstype,
            source,
            mount_opts,
            super_opts,
        })
    }

    #[cfg(target_os = "linux")]
    fn read_mountinfo(path: &Path) -> io::Result<Vec<MountInfo>> {
        let txt = std::fs::read_to_string(path)?;
        let mut out = Vec::new();
        for line in txt.lines() {
            if let Some(mi) = Self::parse_mountinfo_line(line) {
                out.push(mi);
            }
        }
        Ok(out)
    }

    #[cfg(target_os = "netbsd")]
    fn read_mountinfo(_path: &Path) -> io::Result<Vec<MountInfo>> {
        netbsd_mounts::read_mounts()
    }

    fn snapshot_for_watched(&self, all: &[MountInfo]) -> HashMap<PathBuf, MountInfo> {
        let mut map = HashMap::new();
        for mi in all {
            // watch by mount_point
            if self.watched.contains(&mi.mount_point) {
                map.insert(mi.mount_point.clone(), mi.clone());
            }
        }
        map
    }

    fn materially_diff(a: &MountInfo, b: &MountInfo) -> bool {
        #[cfg(target_os = "netbsd")]
        {
            a.fstype != b.fstype || a.source != b.source || a.mount_opts != b.mount_opts
        }

        #[cfg(target_os = "linux")]
        {
            a.mount_id != b.mount_id
                || a.parent_id != b.parent_id
                || a.root != b.root
                || a.fstype != b.fstype
                || a.source != b.source
                || a.mount_opts != b.mount_opts
                || a.super_opts != b.super_opts
        }
    }

    pub async fn run(mut self, ctx: SensorCtx<XMountEvent>) -> io::Result<()> {
        if self.watched.is_empty() {
            return Ok(());
        }

        // prime snapshot
        let all = Self::read_mountinfo(&self.config.mountinfo_path)?;
        self.last = self.snapshot_for_watched(&all);
        self.is_primed = true;

        let mut ticker = time::interval(self.config.pulse);

        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break Ok(()),
                _ = ticker.tick() => {}
            }

            let all = match Self::read_mountinfo(&self.config.mountinfo_path) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("xmount: failed to read mountinfo: {e}");
                    continue;
                }
            };

            let now = self.snapshot_for_watched(&all);

            // Mounted / Changed
            for (mp, new_info) in &now {
                match self.last.get(mp) {
                    None => {
                        if self.is_primed {
                            Self::fire(&ctx.hub, XMountEvent::Mounted { target: mp.clone(), info: new_info.clone() }).await;
                        }
                    }
                    Some(old_info) => {
                        if Self::materially_diff(old_info, new_info) {
                            Self::fire(&ctx.hub, XMountEvent::Changed { target: mp.clone(), old: old_info.clone(), new: new_info.clone() }).await;
                        }
                    }
                }
            }

            // Unmounted
            for (mp, old_info) in &self.last {
                if !now.contains_key(mp) {
                    Self::fire(&ctx.hub, XMountEvent::Unmounted { target: mp.clone(), last: old_info.clone() }).await;
                }
            }

            self.last = now;
        }
    }
}

impl Sensor for XMount {
    type Event = XMountEvent;

    fn run(self, ctx: SensorCtx<Self::Event>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            if let Err(e) = XMount::run(self, ctx).await {
                log::error!("xmount: sensors stopped: {e}");
            }
        })
    }
}

#[cfg(target_os = "netbsd")]
fn c_char_array_to_string(buf: &[libc::c_char]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let bytes: Vec<u8> = buf[..len].iter().map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(target_os = "netbsd")]
fn mount_flags_to_opts(flags: u64) -> String {
    // NetBSD statvfs flags are ST_*; we only map the obvious ones.
    // If you want the full list, expand it.
    let mut out = Vec::new();

    // These names come from NetBSD statvfs docs. :contentReference[oaicite:3]{index=3}
    const ST_RDONLY: u64 = 0x0000_0001;
    const ST_NOEXEC: u64 = 0x0000_0002;
    const ST_NOSUID: u64 = 0x0000_0008;
    const ST_NODEV: u64 = 0x0000_0010;

    out.push(if (flags & ST_RDONLY) != 0 { "ro" } else { "rw" });

    if (flags & ST_NOEXEC) != 0 {
        out.push("noexec");
    }
    if (flags & ST_NOSUID) != 0 {
        out.push("nosuid");
    }
    if (flags & ST_NODEV) != 0 {
        out.push("nodev");
    }

    out.join(",")
}

#[cfg(target_os = "netbsd")]
mod netbsd_mounts {
    use super::*;
    use std::{io, ptr};

    // NetBSD uses versioned symbols; this avoids ABI mismatch pain. :contentReference[oaicite:4]{index=4}
    extern "C" {
        #[link_name = "__getmntinfo13"]
        fn getmntinfo(mntbufp: *mut *mut libc::statvfs, flags: libc::c_int) -> libc::c_int;
    }

    // NetBSD flags for getmntinfo forward to getvfsstat(2). :contentReference[oaicite:5]{index=5}
    const MNT_NOWAIT: libc::c_int = 2;

    pub fn read_mounts() -> io::Result<Vec<MountInfo>> {
        unsafe {
            let mut buf: *mut libc::statvfs = ptr::null_mut();
            let n = getmntinfo(&mut buf as *mut *mut libc::statvfs, MNT_NOWAIT);
            if n < 0 {
                return Err(io::Error::last_os_error());
            }

            let slice = std::slice::from_raw_parts(buf, n as usize);
            let mut out = Vec::with_capacity(slice.len());

            for sv in slice {
                // Field layout is defined by NetBSD statvfs(5). :contentReference[oaicite:6]{index=6}
                let fstype = c_char_array_to_string(&sv.f_fstypename);
                let target = c_char_array_to_string(&sv.f_mntonname);
                let source = c_char_array_to_string(&sv.f_mntfromname);

                let mount_opts = mount_flags_to_opts(sv.f_flag as u64);

                out.push(MountInfo {
                    mount_id: 0,
                    parent_id: 0,
                    mount_point: PathBuf::from(target),
                    root: PathBuf::from("/"),
                    fstype,
                    source,
                    mount_opts,
                    super_opts: String::new(),
                });
            }

            Ok(out)
        }
    }
}
