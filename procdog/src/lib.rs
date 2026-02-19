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
}

impl Default for ProcDogConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcState {
    Running { pid: i32 },
    Stopped,
}

pub struct ProcDog {
    watched: HashSet<String>,
    ignored: HashSet<String>,
    state: HashMap<String, ProcState>,

    config: ProcDogConfig,
    callbacks: Vec<Arc<dyn ProcDogCallback>>,
    results_tx: Option<mpsc::Sender<CallbackResult>>,

    is_primed: bool,
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
            is_primed: false,
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

                if let Some((pid, _)) = procs.iter().find(|(_, n)| n == name) {
                    self.state
                        .insert(name.clone(), ProcState::Running { pid: *pid });
                } else {
                    self.state.insert(name.clone(), ProcState::Stopped);
                }
            }
        }

        self.is_primed = true;
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

            let found = procs.iter().find(|(_, n)| n == name);

            let current = self.state.get(name).copied().unwrap_or(ProcState::Stopped);

            match (current, found) {
                (ProcState::Stopped, Some((pid, _))) => {
                    self.state
                        .insert(name.clone(), ProcState::Running { pid: *pid });

                    self.fire(ProcDogEvent::Appeared {
                        name: name.clone(),
                        pid: *pid,
                    })
                    .await;
                }

                (ProcState::Running { pid: old_pid }, Some((new_pid, _)))
                    if old_pid != *new_pid =>
                {
                    self.state
                        .insert(name.clone(), ProcState::Running { pid: *new_pid });

                    self.fire(ProcDogEvent::Appeared {
                        name: name.clone(),
                        pid: *new_pid,
                    })
                    .await;
                }

                (ProcState::Running { .. }, None) => {
                    self.state.insert(name.clone(), ProcState::Stopped);

                    self.fire(ProcDogEvent::Disappeared { name: name.clone() })
                        .await;
                }

                _ => {}
            }
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
