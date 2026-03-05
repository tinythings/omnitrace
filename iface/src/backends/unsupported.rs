use crate::IfaceBackend;
use crate::events::IfaceEvent;
use std::io;
use std::time::Duration;

pub struct UnsupportedBackend;

#[async_trait::async_trait]
impl IfaceBackend for UnsupportedBackend {
    async fn next_event(&mut self, _timeout: Duration) -> io::Result<Option<IfaceEvent>> {
        Ok(None)
    }
}
