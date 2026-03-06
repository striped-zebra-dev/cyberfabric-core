//! Transactional outbox for reliable asynchronous message production.
//!
//! # Architecture
//!
//! Four-stage pipeline: **incoming → sequencer → outgoing → processor**.
//!
//! 1. **Enqueue** — messages are written atomically within business transactions
//!    to the `incoming` table via [`Outbox::enqueue()`].
//! 2. **Sequencer** — a background task claims incoming rows, assigns
//!    per-partition sequence numbers, and writes to the `outgoing` table.
//! 3. **Processor** — one long-lived task per partition reads from `outgoing`,
//!    dispatches to the registered handler, and acks via cursor advance
//!    (append-only — no deletes on the hot path).
//! 4. **Reaper** — when a partition is idle, the processor bulk-deletes
//!    processed outgoing and body rows.
//!
//! # Processing modes
//!
//! - **Transactional** — handler runs inside the DB transaction holding the
//!   partition lock. Provides exactly-once semantics within the database.
//! - **Decoupled** — handler runs outside any transaction, with lease-based
//!   locking. Provides at-least-once delivery; handlers must be idempotent.
//!
//! # Usage
//!
//! ```ignore
//! let handle = Outbox::builder(db)
//!     .poll_interval(Duration::from_millis(100))
//!     .queue("orders", Partitions::of(4))
//!         .decoupled(my_handler)
//!     .start().await?;
//! // ... enqueue via handle.outbox() ...
//! handle.stop().await;
//! ```

mod builder;
mod core;
mod dead_letter;
mod dialect;
mod handler;
mod manager;
mod migrations;
mod processor;
mod sequencer;
mod strategy;
mod types;

#[cfg(test)]
#[cfg(feature = "sqlite")]
#[cfg_attr(coverage_nightly, coverage(off))]
mod integration_tests;

pub use builder::QueueBuilder;
pub use core::Outbox;
pub use dead_letter::{DeadLetterFilter, DeadLetterItem, DeadLetterOps};
pub use handler::{
    EachMessage, Handler, HandlerResult, OutboxMessage, SingleHandler, SingleTransactionalHandler,
    TransactionalHandler,
};
pub use manager::{OutboxBuilder, OutboxHandle};
pub use migrations::outbox_migrations;
pub use sequencer::{SequenceBatchResult, Sequencer};
pub use types::{
    OutboxConfig, OutboxError, OutboxItemId, Partitions, QueueConfig, SequencerConfig,
};

// Internal re-exports for tests and internal modules
