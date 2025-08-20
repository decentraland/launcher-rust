use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::Mutex;

use segment::Batcher;
use segment::message::BatchMessage;
use serde_json::Value;

use crate::analytics::infrastructure::event_queue::AnalyticsEventQueue;

pub struct QueuedBatcher {
    queue: Arc<Mutex<dyn AnalyticsEventQueue + Send>>,
    batcher: Batcher,
    context: Option<Value>,
}

impl QueuedBatcher {
    pub fn new(queue: Arc<Mutex<dyn AnalyticsEventQueue + Send>>, context: Option<Value>) -> Self {
        Self {
            queue,
            batcher: Batcher::new(context.clone()),
            context,
        }
    }

    // Enqueues the event, doesn't send instantly
    pub fn push(&mut self, msg: impl Into<BatchMessage>) -> Result<Option<BatchMessage>> {
        self.batcher.push(msg).context("Cannot push message")
    }

    pub async fn flush(&mut self) -> Result<()> {
        if self.batcher.is_empty() {
            return Ok(());
        }

        let batcher = std::mem::replace(&mut self.batcher, Batcher::new(self.context.clone()));
        let message = batcher.into_message();
        self.queue.lock().await.enque(message)
    }
}
