use crate::{Error, Result};
use serde_json::Value;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{sync::mpsc, time::Instant};
use tokio_util::sync::CancellationToken;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Cancellation, an absolute deadline and an optional bounded Responses event sink.
#[derive(Clone, Debug)]
pub struct RunContext {
    pub cancellation: CancellationToken,
    pub deadline: Instant,
    pub events: Option<mpsc::Sender<Value>>,
    emitted: Arc<AtomicU64>,
}

impl Default for RunContext {
    fn default() -> Self {
        Self::with_timeout(Duration::from_secs(300))
    }
}

impl RunContext {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            cancellation: CancellationToken::new(),
            deadline: Instant::now() + timeout,
            events: None,
            emitted: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn with_events(mut self, events: mpsc::Sender<Value>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn check(&self) -> Result<()> {
        if self.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(Error::Timeout);
        }
        Ok(())
    }

    pub async fn wait<T>(&self, future: impl Future<Output = Result<T>>) -> Result<T> {
        self.check()?;
        tokio::select! {
            biased;
            () = self.cancellation.cancelled() => Err(Error::Cancelled),
            result = tokio::time::timeout_at(self.deadline, future) => result.map_err(|_| Error::Timeout)?,
        }
    }

    pub async fn emit(&self, event: Value) -> Result<()> {
        self.emitted.fetch_add(1, Ordering::Relaxed);
        if let Some(sender) = &self.events {
            self.wait(async { sender.send(event).await.map_err(|_| Error::Cancelled) })
                .await?;
        }
        Ok(())
    }

    pub(crate) fn emitted(&self) -> u64 {
        self.emitted.load(Ordering::Relaxed)
    }
}
