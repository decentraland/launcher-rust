use anyhow::{Result, anyhow};
use log::error;
use std::{sync::Arc, time::Duration};

use segment::Client;
use tokio::{sync::Mutex, task::JoinHandle, time::sleep};

use crate::analytics::infrastructure::event_queue::{AnalyticsEvent, AnalyticsEventQueue};

const DEFAULT_PROCESS_DELAY_AFTER_ERROR: Duration = Duration::from_millis(200);

pub struct AnalyticsEventSendDaemon<TClient: Client + Send> {
    queue: Arc<Mutex<dyn AnalyticsEventQueue + Send>>,
    process_delay: Duration,
    write_key: String,
    client: Arc<Mutex<TClient>>,
    task: Option<JoinHandle<()>>,
}

impl<TClient: Client + Send + 'static> AnalyticsEventSendDaemon<TClient> {
    pub fn start(&mut self) {
        self.stop();

        let client = self.client.clone();
        let queue = self.queue.clone();
        let write_key = self.write_key.clone();
        let process_delay = self.process_delay;

        let handle = tokio::spawn(async move {
            loop {
                let result = Self::send(queue.clone(), client.clone(), write_key.clone()).await;
                if let Err(e) = result {
                    error!("Error executing send loop: {:#?}", e);
                    sleep(process_delay).await;
                }
            }
        });

        self.task = Some(handle);
    }
}

impl<TClient: Client + Send> AnalyticsEventSendDaemon<TClient> {
    pub fn new(
        queue: Arc<Mutex<dyn AnalyticsEventQueue + Send>>,
        process_delay: Option<Duration>,
        write_key: String,
        client: TClient,
    ) -> Self {
        Self {
            queue,
            process_delay: process_delay.unwrap_or(DEFAULT_PROCESS_DELAY_AFTER_ERROR),
            write_key,
            client: Arc::new(Mutex::new(client)),
            task: None,
        }
    }

    pub fn stop(&mut self) {
        if let Some(task) = &self.task {
            // TODO use notify for graceful cancellation
            task.abort();
            self.task = None;
        }
    }

    async fn send(
        queue: Arc<Mutex<dyn AnalyticsEventQueue + Send>>,
        client: Arc<Mutex<TClient>>,
        write_key: String,
    ) -> Result<()> {
        if let Some(event) = queue.lock().await.peek() {
            let AnalyticsEvent { id, message } = event;
            if let Err(e) = client
                .lock()
                .await
                .send(write_key, message.into_owned())
                .await
            {
                Err(anyhow!(
                    "Cannot send event in daemon loop due error (will retry): {:#?}",
                    e
                ))
            } else {
                queue.lock().await.consume(id);
                Ok(())
            }
        } else {
            Ok(())
        }
    }
}

impl<TClient: Client + Send> Drop for AnalyticsEventSendDaemon<TClient> {
    fn drop(&mut self) {
        self.stop();
    }
}
