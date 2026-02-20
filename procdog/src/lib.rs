pub mod backends;
pub mod events;

use crate::events::{CallbackResult, ProcDogCallback, ProcDogEvent};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;

#[async_trait::async_trait]
pub trait ProcBackend: Send + Sync {
    async fn list(&self) -> std::io::Result<Vec<(i32, String)>>;
}

pub struct ProcDogConfig {
    interval: Duration,
    emit_missing_on_start: bool,
}

impl Default for ProcDogConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            emit_missing_on_start: false,
        }
    }
}

impl ProcDogConfig {
    pub fn interval(mut self, d: Duration) -> Self {
        self.interval = d;
        self
    }

    fn get_interval(&self) -> Duration {
        self.interval
    }

    pub fn emit_missing_on_start(mut self, on: bool) -> Self {
        self.emit_missing_on_start = on;
        self
    }
}

pub struct ProcDog {
    watched: HashSet<String>,
    ignored: HashSet<String>,

    // name -> active PIDs
    state: HashMap<String, HashSet<i32>>,

    config: ProcDogConfig,
    callbacks: Vec<Arc<dyn ProcDogCallback>>,
    results_tx: Option<mpsc::Sender<CallbackResult>>,
    backend: Arc<dyn ProcBackend>,
}

impl ProcDog {
    pub fn new(cfg: Option<ProcDogConfig>) -> Self {
        Self {
            watched: HashSet::new(),
            ignored: HashSet::new(),
            state: HashMap::new(),
            config: cfg.unwrap_or_default(),
            callbacks: Vec::new(),
            results_tx: None,
            backend: Arc::new(backends::stps::PsBackend),
        }
    }

    pub fn set_backend<B>(&mut self, backend: B)
    where
        B: ProcBackend + 'static,
    {
        self.backend = Arc::new(backend);
    }

    pub fn watch<S: Into<String>>(&mut self, name: S) {
        self.watched.insert(name.into());
    }

    pub fn ignore<S: Into<String>>(&mut self, pattern: S) {
        self.ignored.insert(pattern.into());
    }

    pub fn add_callback<C: ProcDogCallback>(&mut self, cb: C) {
        self.callbacks.push(Arc::new(cb));
    }

    pub fn set_callback_channel(&mut self, tx: mpsc::Sender<CallbackResult>) {
        self.results_tx = Some(tx);
    }

    async fn fire(&self, ev: ProcDogEvent) {
        for cb in &self.callbacks {
            if cb.mask().matches(&ev)
                && let Some(r) = cb.call(&ev).await
                && let Some(tx) = &self.results_tx
            {
                let _ = tx.send(r).await;
            }
        }
    }

    async fn prime(&mut self) {
        if let Ok(procs) = self.backend.list().await {
            for name in &self.watched {
                if self.ignored.contains(name) {
                    continue;
                }

                let pids: HashSet<i32> = procs
                    .iter()
                    .filter(|(_, n)| n == name)
                    .map(|(pid, _)| *pid)
                    .collect();

                if self.config.emit_missing_on_start && pids.is_empty() {
                    self.fire(ProcDogEvent::Missing { name: name.clone() })
                        .await;
                }

                self.state.insert(name.clone(), pids);
            }
        }
    }

    async fn tick_once(&mut self) {
        let procs = match self.backend.list().await {
            Ok(p) => p,
            Err(_) => return,
        };

        for name in &self.watched {
            if self.ignored.contains(name) {
                continue;
            }

            let current: HashSet<i32> = procs
                .iter()
                .filter(|(_, n)| n == name)
                .map(|(pid, _)| *pid)
                .collect();

            let previous = self.state.get(name).cloned().unwrap_or_default();

            // Determine diffs without holding mutable borrow
            let appeared: Vec<i32> = current.difference(&previous).copied().collect();

            let disappeared: Vec<i32> = previous.difference(&current).copied().collect();

            // Fire events
            for pid in &appeared {
                self.fire(ProcDogEvent::Appeared {
                    name: name.clone(),
                    pid: *pid,
                })
                .await;
            }

            for pid in &disappeared {
                self.fire(ProcDogEvent::Disappeared {
                    name: name.clone(),
                    pid: *pid,
                })
                .await;
            }

            // Now update state
            self.state.insert(name.clone(), current);
        }
    }

    pub async fn run(mut self) {
        self.prime().await;

        let mut ticker = tokio::time::interval(self.config.get_interval());

        loop {
            ticker.tick().await;
            self.tick_once().await;
        }
    }
}
