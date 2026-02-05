use hashbrown::HashMap;
use std::path::{Path, PathBuf};
use tokio::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirStamp {
    mtime_ns: u128,
}

fn is_under(path: &std::path::Path, dir: &std::path::Path) -> bool {
    path.strip_prefix(dir).is_ok()
}

pub struct FileScream {
    roots: Vec<PathBuf>,
    state: HashMap<PathBuf, blake3::Hash>,
    interval: Duration,
    dir_state: HashMap<PathBuf, DirStamp>,
    primed: bool,
}

impl Default for FileScream {
    fn default() -> Self {
        Self::new()
    }
}

impl FileScream {
    pub fn new() -> Self {
        Self { roots: Vec::new(), state: HashMap::new(), interval: Duration::from_secs(3), dir_state: HashMap::new(), primed: false }
    }

    pub fn watch<P: AsRef<Path>>(&mut self, path: P) {
        self.roots.push(path.as_ref().to_path_buf());
    }

    fn mtime_ns(meta: &std::fs::Metadata) -> u128 {
        meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)
    }

    fn scan(
        roots: &[PathBuf], prev_dir_state: &mut HashMap<PathBuf, DirStamp>, prev_files: &HashMap<PathBuf, blake3::Hash>,
    ) -> HashMap<PathBuf, blake3::Hash> {
        let mut out = HashMap::new();

        for root in roots {
            let mut stack = vec![root.clone()];

            while let Some(path) = stack.pop() {
                let meta = match std::fs::symlink_metadata(&path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                if meta.is_dir() {
                    let stamp = DirStamp { mtime_ns: Self::mtime_ns(&meta) };
                    let old = prev_dir_state.get(&path).copied();

                    prev_dir_state.insert(path.clone(), stamp);

                    if old.is_some() && old == Some(stamp) && path != *root {
                        for (p, h) in prev_files.iter() {
                            if is_under(p.as_path(), path.as_path()) {
                                out.insert(p.clone(), *h);
                            }
                        }
                        continue;
                    }

                    let rd = match std::fs::read_dir(&path) {
                        Ok(rd) => rd,
                        Err(_) => continue,
                    };

                    for ent in rd.flatten() {
                        stack.push(ent.path());
                    }
                } else if meta.is_file() {
                    let mut h = blake3::Hasher::new();
                    h.update(&meta.len().to_le_bytes());
                    h.update(&Self::mtime_ns(&meta).to_le_bytes());
                    out.insert(path, h.finalize());
                } else {
                    // XXX: Add symlinks/devices/etc
                }
            }
        }

        out
    }

    async fn scan_blocking(&mut self) -> (HashMap<PathBuf, blake3::Hash>, HashMap<PathBuf, DirStamp>) {
        let roots = self.roots.clone();
        let dir_state = std::mem::take(&mut self.dir_state);
        let prev_files = self.state.clone();

        tokio::task::spawn_blocking(move || {
            let mut ds = dir_state;
            let files = Self::scan(&roots, &mut ds, &prev_files);
            (files, ds)
        })
        .await
        .expect("scan task panicked")
    }

    pub async fn run(mut self) {
        let (files, dirs) = self.scan_blocking().await;
        self.state = files;
        self.dir_state = dirs;
        self.primed = true;

        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            let (new_files, new_dir_state) = self.scan_blocking().await;
            self.dir_state = new_dir_state;

            for (path, new_hash) in &new_files {
                match self.state.get(path) {
                    None => println!("file {:?} created", path),
                    Some(old_hash) if old_hash != new_hash => println!("file {:?} changed", path),
                    _ => {}
                }
            }

            for path in self.state.keys() {
                if !new_files.contains_key(path) {
                    println!("file {:?} removed", path);
                }
            }

            self.state = new_files;
        }
    }
}
