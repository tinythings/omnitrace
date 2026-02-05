use filescream::{FileScream, FileScriptConfig};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let mut fs = FileScream::new(Some(FileScriptConfig::default().pulse(Duration::from_secs(1))));

    fs.watch("/tmp");
    tokio::spawn(fs.run());

    // emulate your app doing other work
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
