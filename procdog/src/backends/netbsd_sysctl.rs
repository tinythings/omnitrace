use std::{io, mem};

pub struct NetBsdSysctlBackend;

#[async_trait::async_trait]
impl crate::ProcBackend for NetBsdSysctlBackend {
    async fn list(&self) -> io::Result<Vec<(i32, String)>> {
        unsafe {
            let ksz = mem::size_of::<libc::kinfo_proc2>();
            if ksz == 0 {
                return Ok(Vec::new());
            }

            let mut mib = [
                libc::CTL_KERN,
                libc::KERN_PROC2,
                libc::KERN_PROC_ALL,
                0,                  // arg
                ksz as libc::c_int, // element size
                0,                  // element count (set below)
            ];

            // Probe required size
            let mut size: libc::size_t = 0;
            if libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as libc::c_uint,
                std::ptr::null_mut(),
                &mut size as *mut _,
                std::ptr::null_mut(),
                0,
            ) != 0
            {
                return Err(io::Error::last_os_error());
            }

            if size == 0 {
                return Ok(Vec::new());
            }

            // NetBSD expects mib[5] = number of entries for KERN_PROC2
            let mut count = (size as usize) / ksz;
            if count == 0 {
                count = 1;
            }

            let mut tries = 0;
            let mut buf: Vec<u8>;
            let mut out_size: libc::size_t;

            loop {
                tries += 1;
                mib[5] = count as libc::c_int;

                let alloc_bytes = (count + 64) * ksz;
                buf = vec![0u8; alloc_bytes];
                out_size = alloc_bytes as libc::size_t;

                let rc = libc::sysctl(
                    mib.as_mut_ptr(),
                    mib.len() as libc::c_uint,
                    buf.as_mut_ptr() as *mut _,
                    &mut out_size as *mut _,
                    std::ptr::null_mut(),
                    0,
                );

                if rc == 0 {
                    // truncate to what kernel actually wrote
                    buf.truncate(out_size as usize);
                    break;
                }

                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::ENOMEM) && tries < 8 {
                    count = count.saturating_mul(2).saturating_add(128);
                    continue;
                }

                return Err(err);
            }

            // Only parse full records
            let usable = (buf.len() / ksz) * ksz;
            buf.truncate(usable);

            let n = buf.len() / ksz;
            let mut out = Vec::with_capacity(n);

            for i in 0..n {
                let base = buf.as_ptr().add(i * ksz) as *const libc::kinfo_proc2;

                // Avoid UB: buffer is u8-aligned, so read as unaligned
                let kp: libc::kinfo_proc2 = std::ptr::read_unaligned(base);

                let pid = kp.p_pid as i32;

                let comm_ptr = kp.p_comm.as_ptr() as *const libc::c_char;
                let comm = std::ffi::CStr::from_ptr(comm_ptr)
                    .to_string_lossy()
                    .trim()
                    .to_string();

                if !comm.is_empty() {
                    out.push((pid, comm));
                }
            }

            Ok(out)
        }
    }
}
