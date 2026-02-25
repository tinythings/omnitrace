use std::{future::Future, pin::Pin, sync::Arc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::callbacks::CallbackHub;

pub trait Sensor: Send + 'static {
    type Event: Send + Sync + 'static;

    fn run(self, ctx: SensorCtx<Self::Event>) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

pub struct SensorCtx<E>
where
    E: Send + Sync + 'static,
{
    pub cancel: CancellationToken,
    pub hub: Arc<CallbackHub<E>>,
}

#[derive(Clone)]
pub struct SensorHandle {
    cancel: CancellationToken,
}

impl SensorHandle {
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
    pub async fn cancelled(&self) {
        self.cancel.cancelled().await;
    }
}

impl<E> SensorCtx<E>
where
    E: Send + Sync + 'static,
{
    pub fn new(hub: Arc<CallbackHub<E>>) -> (Self, SensorHandle) {
        let cancel = CancellationToken::new();
        let handle = SensorHandle { cancel: cancel.clone() };
        (Self { cancel, hub }, handle)
    }
}

pub fn spawn_sensor<S>(sensor: S, hub: Arc<CallbackHub<S::Event>>) -> (SensorHandle, JoinHandle<()>)
where
    S: Sensor,
{
    let (ctx, handle) = SensorCtx::new(hub);
    let jh = tokio::spawn(sensor.run(ctx));
    (handle, jh)
}
