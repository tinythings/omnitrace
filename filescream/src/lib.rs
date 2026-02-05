use blake3::Hash;
use hashbrown::HashMap;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use tokio::time::Duration;

pub struct FileScriptConfig {
    pulse: Duration,
}

impl Default for FileScriptConfig {
    fn default() -> Self {
        Self { pulse: Duration::from_secs(3) }
    }
}

impl FileScriptConfig {
    pub fn pulse(mut self, pulse: Duration) -> Self {
        self.pulse = pulse;
        self
    }

    fn get_pulse(&self) -> Duration {
        self.pulse
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirStamp {
    mtime_ns: u128,
}

pub struct FileScream {
    watched: HashSet<PathBuf>,
    ignored: HashSet<String>, // glob patterns
    fstate: HashMap<PathBuf, Hash>,
    dstate: HashMap<PathBuf, DirStamp>,
    config: FileScriptConfig,
    is_primed: bool,
}

impl Default for FileScream {
    fn default() -> Self {
        Self::new(None)
    }
}

impl FileScream {
    pub fn new(config: Option<FileScriptConfig>) -> Self {
        Self {
            watched: HashSet::new(),
            ignored: HashSet::new(),
            fstate: HashMap::new(),
            config: config.unwrap_or_default(),
            dstate: HashMap::new(),
            is_primed: false,
        }
    }

    /// Add a directory to watch. Subdirectories will be watched as well.
    pub fn watch<P: AsRef<Path>>(&mut self, path: P) {
        self.watched.insert(path.as_ref().to_path_buf());
    }

    /// Remove a directory from being watched.
    pub fn unwatch<P: AsRef<Path>>(&mut self, path: P) {
        self.watched.remove(path.as_ref());
    }

    /// Add a glob pattern to ignore. Ignored paths will not be scanned or reported on.
    pub fn ignore<S: Into<String>>(&mut self, pattern: S) {
        self.ignored.insert(pattern.into());
    }

    /// Remove a glob pattern from being ignored.
    pub fn unignore<S: AsRef<str>>(&mut self, pattern: S) {
        self.ignored.remove(pattern.as_ref());
    }

    fn mtime_ns(meta: &std::fs::Metadata) -> u128 {
        meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)
    }

    fn is_under(path: &std::path::Path, dir: &std::path::Path) -> bool {
        path.strip_prefix(dir).is_ok()
    }

    fn scan(roots: &[PathBuf], prev_dir_state: &mut HashMap<PathBuf, DirStamp>, prev_files: &HashMap<PathBuf, Hash>) -> HashMap<PathBuf, Hash> {
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
                            if Self::is_under(p.as_path(), path.as_path()) {
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

    async fn scan_blocking(&mut self) -> (HashMap<PathBuf, Hash>, HashMap<PathBuf, DirStamp>) {
        let roots = self.watched.clone();
        let dir_state = std::mem::take(&mut self.dstate);
        let prev_files = self.fstate.clone();

        tokio::task::spawn_blocking(move || {
            let mut ds = dir_state;
            let files = Self::scan(&roots.iter().cloned().collect::<Vec<_>>(), &mut ds, &prev_files);
            (files, ds)
        })
        .await
        .expect("scan task panicked")
    }

    pub async fn run(mut self) {
        let (files, dirs) = self.scan_blocking().await;
        self.fstate = files;
        self.dstate = dirs;
        self.is_primed = true;

        let mut ticker = tokio::time::interval(self.config.get_pulse());

        loop {
            ticker.tick().await;

            let (new_files, new_dir_state) = self.scan_blocking().await;
            self.dstate = new_dir_state;

            for (path, new_hash) in &new_files {
                match self.fstate.get(path) {
                    None => println!("file {:?} created", path),
                    Some(old_hash) if old_hash != new_hash => println!("file {:?} changed", path),
                    _ => {}
                }
            }

            for path in self.fstate.keys() {
                if !new_files.contains_key(path) {
                    println!("file {:?} removed", path);
                }
            }

            self.fstate = new_files;
        }
    }
}
