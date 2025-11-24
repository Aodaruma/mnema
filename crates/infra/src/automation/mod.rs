use std::sync::Arc;

use tokio::sync::{Mutex, mpsc};

use mnema_core::automation::AutomationJob;

#[derive(Clone)]
pub struct InMemoryJobQueue {
    sender: mpsc::Sender<AutomationJob>,
    receiver: Arc<Mutex<mpsc::Receiver<AutomationJob>>>,
}

impl InMemoryJobQueue {
    pub fn new(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    pub async fn enqueue(
        &self,
        job: AutomationJob,
    ) -> Result<(), mpsc::error::SendError<AutomationJob>> {
        self.sender.send(job).await
    }

    pub async fn dequeue(&self) -> Option<AutomationJob> {
        let mut rx = self.receiver.lock().await;
        rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mnema_core::automation::AutomationJobKind;

    #[tokio::test]
    async fn enqueue_and_dequeue_in_order() {
        let queue = InMemoryJobQueue::new(4);
        queue
            .enqueue(AutomationJob {
                kind: AutomationJobKind::InboxClassify,
            })
            .await
            .unwrap();
        queue
            .enqueue(AutomationJob {
                kind: AutomationJobKind::WeeklyReviewPrep,
            })
            .await
            .unwrap();

        let first = queue.dequeue().await.unwrap();
        let second = queue.dequeue().await.unwrap();
        assert_eq!(first.kind, AutomationJobKind::InboxClassify);
        assert_eq!(second.kind, AutomationJobKind::WeeklyReviewPrep);
    }
}
