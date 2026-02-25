use crate::ProcBackend;
pub struct PsBackend;

#[async_trait::async_trait]
impl ProcBackend for PsBackend {
    async fn list(&self) -> std::io::Result<Vec<(i32, String)>> {
        use tokio::process::Command;

        let out = Command::new("ps")
            .args(["-ax", "-o", "pid=", "-o", "comm="])
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut result = Vec::new();

        for line in stdout.lines().skip(1) {
            let mut parts = line.trim().split_whitespace();
            if let (Some(pid), Some(name)) = (parts.next(), parts.next()) {
                if let Ok(pid) = pid.parse::<i32>() {
                    result.push((pid, name.to_string()));
                }
            }
        }

        Ok(result)
    }
}
