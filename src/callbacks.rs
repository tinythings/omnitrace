use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;

/// What callbacks can optionally return (goes to the results channel).
pub type CallbackResult = Value;

/// A generic async callback over event type `E`.
#[async_trait]
pub trait Callback<E>: Send + Sync {
    /// Return a bitmask defining which events you care about.
    fn mask(&self) -> u64;

    /// Called when an event fires.
    /// Return Some(Value) to send it to the result channel, or None to ignore.
    async fn call(&self, ev: &E) -> Option<CallbackResult>;
}

/// Shared callback registry (order-preserving) + optional result channel.
#[derive(Default)]
pub struct CallbackHub<E> {
    callbacks: Vec<Arc<dyn Callback<E>>>,
    results_tx: Option<mpsc::Sender<CallbackResult>>,
}

impl<E> CallbackHub<E> {
    pub fn new() -> Self {
        Self {
            callbacks: Vec::new(),
            results_tx: None,
        }
    }

    pub fn add<C: Callback<E> + 'static>(&mut self, cb: C) {
        self.callbacks.push(Arc::new(cb));
    }

    pub fn set_result_channel(&mut self, tx: mpsc::Sender<CallbackResult>) {
        self.results_tx = Some(tx);
    }

    /// Fire an event to callbacks whose mask matches `ev_mask`.
    pub async fn fire(&self, ev_mask: u64, ev: &E) {
        for cb in &self.callbacks {
            if (cb.mask() & ev_mask) == 0 {
                continue;
            }
            if let Some(r) = cb.call(ev).await {
                if let Some(tx) = &self.results_tx {
                    let _ = tx.send(r).await;
                }
            }
        }
    }
}
