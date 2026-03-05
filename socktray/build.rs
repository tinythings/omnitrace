fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "netbsd" && target_os != "freebsd" {
        return;
    }

    cc::Build::new()
        .file("src/backends/bsd_sysctl.c")
        .warnings(true)
        .compile("socktray_bsd_sysctl");
}
