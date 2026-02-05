use crate::events::{CallbackResult, FileScreamCallback, FileScreamEvent};
use blake3::{Hash, Hasher};
use globset::{Glob, GlobSet, GlobSetBuilder};
use hashbrown::HashMap;
use std::{
    collections::HashSet,
    fs::{Metadata, read_dir},
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};
use tokio::{sync::mpsc, task::spawn_blocking, time::Duration};

pub mod events;

#[derive(Clone)]
struct PathGlobMatcher {
    any: GlobSet,
    dir_only: GlobSet,
}

impl Default for PathGlobMatcher {
    fn default() -> Self {
        let empty = GlobSetBuilder::new().build().unwrap();
        Self { any: empty.clone(), dir_only: empty }
    }
}

pub struct FileScreamConfig {
    pulse: Duration,
}

impl Default for FileScreamConfig {
    fn default() -> Self {
        Self { pulse: Duration::from_secs(3) }
    }
}

impl FileScreamConfig {
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
    config: FileScreamConfig,
    callbacks: Vec<Arc<dyn FileScreamCallback>>,
    results_tx: Option<mpsc::Sender<CallbackResult>>,

    is_primed: bool,
    im: PathGlobMatcher,
}

impl Default for FileScream {
    fn default() -> Self {
        Self::new(None)
    }
}

impl FileScream {
    pub fn new(config: Option<FileScreamConfig>) -> Self {
        Self {
            watched: HashSet::new(),
            ignored: HashSet::new(),
            fstate: HashMap::new(),
            dstate: HashMap::new(),

            config: config.unwrap_or_default(),
            is_primed: false,
            im: PathGlobMatcher::default(),
            callbacks: Vec::new(),
            results_tx: None,
        }
    }

    /// Add a directory to watch. Subdirectories will be watched as well.
    pub fn watch<P: AsRef<Path>>(&mut self, path: P) {
        if let Ok(p) = path.as_ref().canonicalize() {
            self.watched.insert(p);
        } else {
            self.watched.insert(path.as_ref().to_path_buf());
        }
    }
    /// Remove a directory from being watched.
    pub fn unwatch<P: AsRef<Path>>(&mut self, path: P) {
        if let Ok(p) = path.as_ref().canonicalize() {
            self.watched.remove(&p);
        } else {
            self.watched.remove(path.as_ref());
        }
    }

    /// Add a glob pattern to ignore. Ignored paths will not be scanned or reported on.
    pub fn ignore<S: Into<String>>(&mut self, pattern: S) {
        self.ignored.insert(pattern.into());
        self.im = self.get_glob_matchers(&self.ignored);
    }

    /// Remove a glob pattern from being ignored.
    pub fn unignore<S: AsRef<str>>(&mut self, pattern: S) {
        self.ignored.remove(pattern.as_ref());
        self.im = self.get_glob_matchers(&self.ignored);
    }

    /// Add a callback to be invoked on file events.
    pub fn add_callback<C: FileScreamCallback>(&mut self, cb: C) {
        self.callbacks.push(Arc::new(cb));
    }

    /// Set a channel to receive callback results. Results are JSON values returned by callbacks that matched an event.
    pub fn set_callback_channel(&mut self, tx: tokio::sync::mpsc::Sender<events::CallbackResult>) {
        self.results_tx = Some(tx);
    }

    fn mtime_ns(meta: &Metadata) -> u128 {
        meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)
    }

    /// Check if a path is under a given directory.
    fn is_under(path: &Path, dir: &Path) -> bool {
        path.strip_prefix(dir).is_ok()
    }

    async fn fire(&self, ev: FileScreamEvent) {
        for cb in &self.callbacks {
            if cb.mask().matches(&ev)
                && let Some(r) = cb.call(&ev).await
                && let Some(tx) = &self.results_tx
            {
                let _ = tx.send(r).await;
            }
        }
    }

    /// Compile glob patterns into matchers for efficient scanning.
    /// Patterns ending with '/' are treated as directory-only, others match files and directories.
    /// Leading '/' anchors the pattern to the filesystem root, otherwise it matches anywhere in the path.
    /// Invalid patterns are ignored.
    fn get_glob_matchers(&self, patterns: &HashSet<String>) -> PathGlobMatcher {
        let mut all = GlobSetBuilder::new();
        let mut dirs = GlobSetBuilder::new();

        for raw in patterns {
            let pat = raw.trim_end_matches('/');

            // semantics:
            //  - leading '/' => anchored at filesystem root (absolute path string starts with '/')
            //  - no leading '/' => match anywhere in path => "**/<pat>"
            let compiled = if pat.starts_with('/') { pat.to_string() } else { format!("**/{}", pat) };

            // Compile glob
            let g = match Glob::new(&compiled) {
                Ok(g) => g,
                Err(_) => continue, // ignore invalid patterns instead of panicking
            };

            if raw.ends_with('/') {
                dirs.add(g);
            } else {
                all.add(g);
            }
        }

        PathGlobMatcher {
            any: all.build().unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap()),
            dir_only: dirs.build().unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap()),
        }
    }

    fn scan(roots: &[PathBuf], ignore: &PathGlobMatcher, dir_state: &mut HashMap<PathBuf, DirStamp>) -> HashMap<PathBuf, Hash> {
        let mut out = HashMap::new();

        for root in roots {
            let mut stack = vec![root.clone()]; // DFS

            while let Some(path) = stack.pop() {
                let meta = match std::fs::symlink_metadata(&path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let is_dir = meta.is_dir();
                let s = path.to_string_lossy();

                // ignore pruning
                if (is_dir && ignore.dir_only.is_match(&*s)) || ignore.any.is_match(&*s) {
                    continue;
                }

                if is_dir {
                    dir_state.insert(path.clone(), DirStamp { mtime_ns: Self::mtime_ns(&meta) });

                    let rd = match read_dir(&path) {
                        Ok(rd) => rd,
                        Err(_) => continue,
                    };

                    for ent in rd.flatten() {
                        stack.push(ent.path());
                    }
                } else if meta.is_file() {
                    let mut h = Hasher::new();
                    h.update(&meta.len().to_le_bytes());
                    h.update(&Self::mtime_ns(&meta).to_le_bytes());
                    out.insert(path, h.finalize());
                } else {
                    // XXX: ignore symlinks/devices/etc for now
                }
            }
        }

        out
    }

    async fn scan_blocking(&mut self) -> (HashMap<PathBuf, Hash>, HashMap<PathBuf, DirStamp>) {
        let roots: Vec<PathBuf> = self.watched.iter().cloned().collect();
        let ignore = self.im.clone();
        let dir_state = std::mem::take(&mut self.dstate);

        spawn_blocking(move || {
            let mut ds = dir_state;
            let files = Self::scan(&roots, &ignore, &mut ds);
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
                if let Some(ev) = match self.fstate.get(path) {
                    None => Some(FileScreamEvent::Created { path: path.clone() }),
                    Some(old_hash) if old_hash != new_hash => Some(FileScreamEvent::Changed { path: path.clone() }),
                    _ => None,
                } {
                    self.fire(ev).await;
                }
            }

            for path in self.fstate.keys() {
                if !new_files.contains_key(path) {
                    self.fire(FileScreamEvent::Removed { path: path.clone() }).await;
                }
            }

            self.fstate = new_files;
        }
    }
}
