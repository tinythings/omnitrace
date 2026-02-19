use std::{io, path::Path};
use tokio::fs;

pub struct LinuxPsBackend;

impl LinuxPsBackend {
    pub fn available() -> bool {
        Path::new("/proc").is_dir()
    }
}

#[async_trait::async_trait]
impl crate::ProcBackend for LinuxPsBackend {
    async fn list(&self) -> io::Result<Vec<(i32, String)>> {
        let mut out = Vec::new();

        let mut rd = fs::read_dir("/proc").await?;
        while let Some(ent) = rd.next_entry().await? {
            let name = ent.file_name();
            let name = name.to_string_lossy();

            // /proc/<pid>
            let Ok(pid) = name.parse::<i32>() else {
                continue;
            };

            // /proc/<pid>/comm is short + stable (not cmdline)
            let comm_path = format!("/proc/{}/comm", pid);
            let Ok(comm) = fs::read_to_string(comm_path).await else {
                continue;
            };

            let comm = comm.trim().to_string();
            if comm.is_empty() {
                continue;
            }

            out.push((pid, comm));
        }

        Ok(out)
    }
}
