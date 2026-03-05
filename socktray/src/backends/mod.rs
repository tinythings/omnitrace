#[cfg(any(target_os = "netbsd", target_os = "freebsd"))]
pub mod bsd_sysctl;
pub mod linux_proc;
pub mod netstat_cmd;
