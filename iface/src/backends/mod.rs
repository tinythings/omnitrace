#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod linux_rtnetlink;

#[cfg(any(target_os = "netbsd", target_os = "freebsd"))]
pub mod bsd_route;

pub mod unsupported;
