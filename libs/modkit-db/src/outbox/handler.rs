use sea_orm::ConnectionTrait;
use tokio_util::sync::CancellationToken;

/// A message read from the outbox for handler processing.
///
/// All messages in a single handler invocation belong to exactly one partition.
/// This is a documented invariant — the processor owns one partition and never
/// mixes messages across partitions in a single call.
pub struct OutboxMessage {
    pub partition_id: i64,
    pub seq: i64,
    pub payload: Vec<u8>,
    pub payload_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// How many times this message has been retried (0 on first attempt).
    /// The handler uses this to decide when to give up and return Reject.
    pub attempts: i16,
}

/// The result of a handler invocation.
pub enum HandlerResult {
    /// All messages processed successfully. The processor advances the cursor
    /// past the last message. Processed outgoing and body rows are cleaned up
    /// asynchronously by the reaper.
    Success,
    /// Transient failure. The cursor is not advanced; the same batch will be
    /// retried with exponential backoff. The `attempts` counter is incremented.
    Retry { reason: String },
    /// Permanent failure. All messages in the batch are moved to the dead-letter
    /// table with inline payload copies. The cursor is advanced past the batch.
    Reject { reason: String },
}

/// Batch handler that runs **decoupled** from any DB transaction.
///
/// **Delivery guarantee:** at-least-once. The processor acquires a lease, reads
/// messages, releases the lock, calls the handler, then opens a new transaction
/// with a lease guard to ack. If the lease expires before ack, another processor
/// may take over and re-deliver the same messages. Handlers must be idempotent.
///
/// **Per-partition invariant:** all messages in a single `handle()` call belong
/// to exactly one partition. The processor owns one partition and never mixes
/// messages across partitions.
///
/// **`cancel` token:** a child token that fires when either (a) the processor
/// is shutting down, or (b) the partition lease is approaching expiry (at 80%
/// of `lease_duration`). Handlers should cooperate by returning `Retry` when
/// cancelled to allow graceful re-processing.
#[async_trait::async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, msgs: &[OutboxMessage], cancel: CancellationToken) -> HandlerResult;
}

/// Batch handler that runs **inside** the DB transaction holding the partition lock.
///
/// **Delivery guarantee:** exactly-once within the database transaction. The
/// handler can perform DB writes atomically with the ack — both commit or both
/// roll back together.
///
/// **Per-partition invariant:** all messages in a single `handle()` call belong
/// to exactly one partition.
///
/// **`cancel` token:** a child of the overall shutdown signal. Unlike decoupled
/// mode, this token is not lease-aware (transactional mode uses row-level locks,
/// not time-based leases).
#[async_trait::async_trait]
pub trait TransactionalHandler: Send + Sync {
    async fn handle(
        &self,
        txn: &dyn ConnectionTrait,
        msgs: &[OutboxMessage],
        cancel: CancellationToken,
    ) -> HandlerResult;
}

/// Single-message handler (decoupled mode).
///
/// Convenience trait for the common case of processing one message at a time.
/// Use via `QueueBuilder::decoupled()`. Internally wrapped with [`EachMessage`].
///
/// Same delivery guarantees and `cancel` semantics as [`Handler`].
#[async_trait::async_trait]
pub trait SingleHandler: Send + Sync {
    async fn handle(&self, msg: &OutboxMessage, cancel: CancellationToken) -> HandlerResult;
}

/// Single-message handler (transactional mode).
///
/// Convenience trait for the common case of processing one message at a time.
/// Use via `QueueBuilder::transactional()`. Internally wrapped with [`EachMessage`].
///
/// Same delivery guarantees and `cancel` semantics as [`TransactionalHandler`].
#[async_trait::async_trait]
pub trait SingleTransactionalHandler: Send + Sync {
    async fn handle(
        &self,
        txn: &dyn ConnectionTrait,
        msg: &OutboxMessage,
        cancel: CancellationToken,
    ) -> HandlerResult;
}

/// Adapter: single-message handler → batch handler.
/// Processes messages one at a time, stops on first non-Success.
/// Propagates the `CancellationToken` to each call via `clone()`.
pub struct EachMessage<H>(pub H);

#[async_trait::async_trait]
impl<H: SingleHandler> Handler for EachMessage<H> {
    async fn handle(&self, msgs: &[OutboxMessage], cancel: CancellationToken) -> HandlerResult {
        for msg in msgs {
            let result = self.0.handle(msg, cancel.clone()).await;
            if !matches!(result, HandlerResult::Success) {
                return result;
            }
        }
        HandlerResult::Success
    }
}

#[async_trait::async_trait]
impl<H: SingleTransactionalHandler> TransactionalHandler for EachMessage<H> {
    async fn handle(
        &self,
        txn: &dyn ConnectionTrait,
        msgs: &[OutboxMessage],
        cancel: CancellationToken,
    ) -> HandlerResult {
        for msg in msgs {
            let result = self.0.handle(txn, msg, cancel.clone()).await;
            if !matches!(result, HandlerResult::Success) {
                return result;
            }
        }
        HandlerResult::Success
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct CountingHandler {
        count: AtomicU32,
    }

    impl CountingHandler {
        fn new() -> Self {
            Self {
                count: AtomicU32::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl SingleHandler for CountingHandler {
        async fn handle(&self, _msg: &OutboxMessage, _cancel: CancellationToken) -> HandlerResult {
            self.count.fetch_add(1, Ordering::Relaxed);
            HandlerResult::Success
        }
    }

    struct FailAtHandler {
        fail_at: u32,
        count: AtomicU32,
        reject: bool,
    }

    #[async_trait::async_trait]
    impl SingleHandler for FailAtHandler {
        async fn handle(&self, _msg: &OutboxMessage, _cancel: CancellationToken) -> HandlerResult {
            let n = self.count.fetch_add(1, Ordering::Relaxed);
            if n == self.fail_at {
                if self.reject {
                    return HandlerResult::Reject {
                        reason: "bad".into(),
                    };
                }
                return HandlerResult::Retry {
                    reason: "transient".into(),
                };
            }
            HandlerResult::Success
        }
    }

    fn make_msg(seq: i64) -> OutboxMessage {
        OutboxMessage {
            partition_id: 1,
            seq,
            payload: vec![],
            payload_type: "test".into(),
            created_at: chrono::Utc::now(),
            attempts: 0,
        }
    }

    #[tokio::test]
    async fn each_message_all_success() {
        let handler = EachMessage(CountingHandler::new());
        let msgs: Vec<OutboxMessage> = (1..=5).map(make_msg).collect();
        let cancel = CancellationToken::new();
        let result = Handler::handle(&handler, &msgs, cancel).await;
        assert!(matches!(result, HandlerResult::Success));
        assert_eq!(handler.0.count.load(Ordering::Relaxed), 5);
    }

    #[tokio::test]
    async fn each_message_stops_on_retry() {
        let handler = EachMessage(FailAtHandler {
            fail_at: 2,
            count: AtomicU32::new(0),
            reject: false,
        });
        let msgs: Vec<OutboxMessage> = (1..=5).map(make_msg).collect();
        let cancel = CancellationToken::new();
        let result = Handler::handle(&handler, &msgs, cancel).await;
        assert!(matches!(result, HandlerResult::Retry { .. }));
        // Processed 0, 1, 2 (failed at index 2) = 3 calls
        assert_eq!(handler.0.count.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn each_message_stops_on_reject() {
        let handler = EachMessage(FailAtHandler {
            fail_at: 1,
            count: AtomicU32::new(0),
            reject: true,
        });
        let msgs: Vec<OutboxMessage> = (1..=5).map(make_msg).collect();
        let cancel = CancellationToken::new();
        let result = Handler::handle(&handler, &msgs, cancel).await;
        assert!(matches!(result, HandlerResult::Reject { .. }));
        assert_eq!(handler.0.count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn each_message_empty_batch() {
        let handler = EachMessage(CountingHandler::new());
        let cancel = CancellationToken::new();
        let result = Handler::handle(&handler, &[], cancel).await;
        assert!(matches!(result, HandlerResult::Success));
        assert_eq!(handler.0.count.load(Ordering::Relaxed), 0);
    }
}
