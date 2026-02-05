use filescream::FileScream;

#[tokio::main]
async fn main() {
    let mut fs = FileScream::new();
    fs.watch("/tmp");

    tokio::spawn(fs.run());

    // emulate "Sysinspect does other shit"
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
