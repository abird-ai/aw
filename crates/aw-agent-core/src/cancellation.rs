//! Cooperative local cancellation with a clonable signal; no task is spawned.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    signal: tokio::sync::watch::Sender<bool>,
}
impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}
impl CancellationToken {
    pub fn new() -> Self {
        let (signal, _) = tokio::sync::watch::channel(false);
        Self { signal }
    }
    pub fn cancel(&self) {
        self.signal.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.signal.borrow()
    }
    pub async fn cancelled(&self) {
        let mut receiver = self.signal.subscribe();
        loop {
            if *receiver.borrow_and_update() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancellation_is_not_lost_before_subscription() {
        let token = CancellationToken::new();
        token.cancel();
        tokio::time::timeout(std::time::Duration::from_millis(20), token.cancelled())
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn clones_share_cancellation() {
        let token = CancellationToken::new();
        let child = token.clone();
        tokio::spawn(async move {
            child.cancel();
        });
        token.cancelled().await;
        assert!(token.is_cancelled());
    }
}
