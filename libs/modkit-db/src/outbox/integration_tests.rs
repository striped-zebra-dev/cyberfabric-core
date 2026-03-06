#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration tests for the transactional outbox subsystem.
//!
//! Organized as narrative chapters that trace complete lifecycle paths.
//! Uses `SQLite` in-memory databases for fast, hermetic testing.
//!
//! Chapter ordering mirrors the pipeline:
//!   1. Registration  →  2. Enqueue  →  3. Sequencer
//!   4. Transactional Processing  →  5. Decoupled Processing
//!   6. Crash Detection & Recovery  →  7. Backoff & Adaptive Batching
//!   8. Reaper  →  9. Dead Letters  →  10. Builder API
//!   11. End-to-End Lifecycle

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sea_orm::{ConnectionTrait, DbBackend, FromQueryResult, Statement, TransactionTrait};
use tokio_util::sync::CancellationToken;

use super::dead_letter::{DeadLetterFilter, DeadLetterOps};
use super::dialect::Dialect;
use super::handler::{
    EachMessage, Handler, HandlerResult, OutboxMessage, SingleHandler, TransactionalHandler,
};
use super::sequencer::Sequencer;
use super::strategy::{
    DecoupledStrategy, ProcessContext, ProcessingStrategy, TransactionalStrategy,
};
use super::{Outbox, OutboxConfig, OutboxError, Partitions, SequencerConfig};
use crate::migration_runner::run_migrations_for_testing;
use crate::outbox::OutboxItemId;
use crate::outbox::QueueConfig;
use crate::{ConnectOpts, Db, connect_db};

// ======================================================================
// Snapshot structs
// ======================================================================

struct TestOutbox {
    outbox: Arc<Outbox>,
    sequencer_notify: Arc<tokio::sync::Notify>,
}

#[derive(Debug)]
struct ProcessorSnapshot {
    processed_seq: i64,
    attempts: i16,
    last_error: Option<String>,
    locked_by: Option<String>,
    locked_until: Option<String>,
}

#[derive(Debug)]
struct OutgoingSnapshot {
    id: i64,
    partition_id: i64,
    body_id: i64,
    seq: i64,
}

#[derive(Debug)]
struct DeadLetterSnapshot {
    id: i64,
    partition_id: i64,
    seq: i64,
    payload: Vec<u8>,
    payload_type: String,
    last_error: Option<String>,
    attempts: i16,
    replayed_at: Option<String>,
}

// ======================================================================
// Layer A — Infrastructure (create resources)
// ======================================================================

async fn setup_db(name: &str) -> Db {
    let url = format!("sqlite:file:{name}?mode=memory&cache=shared");
    let opts = ConnectOpts {
        max_conns: Some(1),
        ..Default::default()
    };
    let db = connect_db(&url, opts).await.expect("connect");
    run_migrations_for_testing(&db, super::outbox_migrations())
        .await
        .expect("migrations");
    db
}

fn make_test_outbox(config: OutboxConfig) -> TestOutbox {
    let notify = Arc::new(tokio::sync::Notify::new());
    TestOutbox {
        outbox: Arc::new(Outbox::new(config, notify.clone())),
        sequencer_notify: notify,
    }
}

fn make_default_test_outbox() -> TestOutbox {
    make_test_outbox(OutboxConfig::default())
}

fn make_sequencer(t: &TestOutbox, config: SequencerConfig) -> Sequencer {
    Sequencer::new(
        config,
        Arc::clone(&t.outbox),
        Arc::clone(&t.sequencer_notify),
    )
}

// ======================================================================
// Layer B — Actions (do things)
// ======================================================================

async fn enqueue_msgs(
    outbox: &Outbox,
    db: &Db,
    queue: &str,
    partition: u32,
    payloads: &[&str],
) -> Vec<OutboxItemId> {
    let conn = db.conn().expect("conn");
    let mut ids = Vec::with_capacity(payloads.len());
    for payload in payloads {
        let id = outbox
            .enqueue(
                &conn,
                queue,
                partition,
                payload.as_bytes().to_vec(),
                "text/plain",
            )
            .await
            .expect("enqueue");
        ids.push(id);
    }
    ids
}

async fn run_sequencer_once(t: &TestOutbox, db: &Db) {
    let seq = make_sequencer(t, SequencerConfig::default());
    seq.sequence_batch(db).await.expect("sequence_batch");
}

async fn enqueue_and_sequence(
    t: &TestOutbox,
    db: &Db,
    queue: &str,
    partition: u32,
    payloads: &[&str],
) -> Vec<OutboxItemId> {
    let ids = enqueue_msgs(&t.outbox, db, queue, partition, payloads).await;
    run_sequencer_once(t, db).await;
    ids
}

async fn simulate_crash(db: &Db, partition_id: i64, lease_secs: i64) {
    let conn = db.sea_internal();
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE modkit_outbox_processor \
         SET locked_by = $1, \
             locked_until = datetime('now', '+' || $2 || ' seconds'), \
             attempts = attempts + 1 \
         WHERE partition_id = $3",
        ["crashed-pod".into(), lease_secs.into(), partition_id.into()],
    ))
    .await
    .expect("simulate_crash");
}

async fn expire_lease(db: &Db, partition_id: i64) {
    let conn = db.sea_internal();
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE modkit_outbox_processor \
         SET locked_until = datetime('now', '-1 seconds') \
         WHERE partition_id = $1",
        [partition_id.into()],
    ))
    .await
    .expect("expire_lease");
}

// ======================================================================
// Layer C — Observations (read state only)
// ======================================================================

async fn count_rows(db: &Db, table: &str) -> i64 {
    #[derive(Debug, FromQueryResult)]
    struct Count {
        cnt: i64,
    }
    let conn = db.sea_internal();
    Count::find_by_statement(Statement::from_string(
        DbBackend::Sqlite,
        format!("SELECT COUNT(*) AS cnt FROM {table}"),
    ))
    .one(&conn)
    .await
    .expect("count query")
    .expect("count row")
    .cnt
}

async fn read_processor_state(db: &Db, partition_id: i64) -> ProcessorSnapshot {
    #[derive(Debug, FromQueryResult)]
    struct Row {
        processed_seq: i64,
        attempts: i16,
        last_error: Option<String>,
        locked_by: Option<String>,
        locked_until: Option<String>,
    }
    let conn = db.sea_internal();
    let row = Row::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT processed_seq, attempts, last_error, locked_by, \
         CAST(locked_until AS TEXT) AS locked_until \
         FROM modkit_outbox_processor WHERE partition_id = $1",
        [partition_id.into()],
    ))
    .one(&conn)
    .await
    .expect("query")
    .expect("processor row");
    ProcessorSnapshot {
        processed_seq: row.processed_seq,
        attempts: row.attempts,
        last_error: row.last_error,
        locked_by: row.locked_by,
        locked_until: row.locked_until,
    }
}

async fn read_outgoing(db: &Db, partition_id: i64) -> Vec<OutgoingSnapshot> {
    #[derive(Debug, FromQueryResult)]
    struct Row {
        id: i64,
        partition_id: i64,
        body_id: i64,
        seq: i64,
    }
    let conn = db.sea_internal();
    Row::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id, partition_id, body_id, seq \
         FROM modkit_outbox_outgoing WHERE partition_id = $1 ORDER BY seq",
        [partition_id.into()],
    ))
    .all(&conn)
    .await
    .expect("query")
    .into_iter()
    .map(|r| OutgoingSnapshot {
        id: r.id,
        partition_id: r.partition_id,
        body_id: r.body_id,
        seq: r.seq,
    })
    .collect()
}

async fn read_dead_letters(db: &Db) -> Vec<DeadLetterSnapshot> {
    #[derive(Debug, FromQueryResult)]
    struct Row {
        id: i64,
        partition_id: i64,
        seq: i64,
        payload: Vec<u8>,
        payload_type: String,
        last_error: Option<String>,
        attempts: i16,
        replayed_at: Option<String>,
    }
    let conn = db.sea_internal();
    Row::find_by_statement(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT id, partition_id, seq, payload, payload_type, last_error, \
         attempts, CAST(replayed_at AS TEXT) AS replayed_at \
         FROM modkit_outbox_dead_letters ORDER BY seq",
    ))
    .all(&conn)
    .await
    .expect("query")
    .into_iter()
    .map(|r| DeadLetterSnapshot {
        id: r.id,
        partition_id: r.partition_id,
        seq: r.seq,
        payload: r.payload,
        payload_type: r.payload_type,
        last_error: r.last_error,
        attempts: r.attempts,
        replayed_at: r.replayed_at,
    })
    .collect()
}

async fn read_partition_sequence(db: &Db, partition_id: i64) -> i64 {
    #[derive(Debug, FromQueryResult)]
    struct Row {
        sequence: i64,
    }
    let conn = db.sea_internal();
    Row::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT sequence FROM modkit_outbox_partitions WHERE id = $1",
        [partition_id.into()],
    ))
    .one(&conn)
    .await
    .expect("query")
    .expect("partition row")
    .sequence
}

async fn poll_until<F, Fut>(f: F, timeout_ms: u64)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if f().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "poll_until timed out after {timeout_ms}ms"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// ======================================================================
// Test handlers
// ======================================================================

struct CountingSuccessHandler {
    count: Arc<AtomicU32>,
}

#[async_trait::async_trait]
impl Handler for CountingSuccessHandler {
    async fn handle(&self, msgs: &[OutboxMessage], _cancel: CancellationToken) -> HandlerResult {
        #[allow(clippy::cast_possible_truncation)]
        self.count.fetch_add(msgs.len() as u32, Ordering::Relaxed);
        HandlerResult::Success
    }
}

struct CountingSingleHandler {
    count: Arc<AtomicU32>,
}

#[async_trait::async_trait]
impl SingleHandler for CountingSingleHandler {
    async fn handle(&self, _msg: &OutboxMessage, _cancel: CancellationToken) -> HandlerResult {
        self.count.fetch_add(1, Ordering::Relaxed);
        HandlerResult::Success
    }
}

struct AlwaysRetryHandler;

#[async_trait::async_trait]
impl SingleHandler for AlwaysRetryHandler {
    async fn handle(&self, _msg: &OutboxMessage, _cancel: CancellationToken) -> HandlerResult {
        HandlerResult::Retry {
            reason: "transient failure".into(),
        }
    }
}

struct AlwaysRejectHandler;

#[async_trait::async_trait]
impl SingleHandler for AlwaysRejectHandler {
    async fn handle(&self, _msg: &OutboxMessage, _cancel: CancellationToken) -> HandlerResult {
        HandlerResult::Reject {
            reason: "permanently bad".into(),
        }
    }
}

struct AttemptsRecorder {
    seen_attempts: Arc<Mutex<Vec<i16>>>,
}

#[async_trait::async_trait]
impl SingleHandler for AttemptsRecorder {
    async fn handle(&self, msg: &OutboxMessage, _cancel: CancellationToken) -> HandlerResult {
        self.seen_attempts.lock().unwrap().push(msg.attempts);
        HandlerResult::Success
    }
}

struct CountingTxHandler {
    count: Arc<AtomicU32>,
}

#[async_trait::async_trait]
impl TransactionalHandler for CountingTxHandler {
    async fn handle(
        &self,
        _txn: &dyn ConnectionTrait,
        msgs: &[OutboxMessage],
        _cancel: CancellationToken,
    ) -> HandlerResult {
        #[allow(clippy::cast_possible_truncation)]
        self.count.fetch_add(msgs.len() as u32, Ordering::Relaxed);
        HandlerResult::Success
    }
}

struct AlwaysRetryTxHandler;

#[async_trait::async_trait]
impl TransactionalHandler for AlwaysRetryTxHandler {
    async fn handle(
        &self,
        _txn: &dyn ConnectionTrait,
        _msgs: &[OutboxMessage],
        _cancel: CancellationToken,
    ) -> HandlerResult {
        HandlerResult::Retry {
            reason: "transient tx failure".into(),
        }
    }
}

struct AlwaysRejectTxHandler;

#[async_trait::async_trait]
impl TransactionalHandler for AlwaysRejectTxHandler {
    async fn handle(
        &self,
        _txn: &dyn ConnectionTrait,
        _msgs: &[OutboxMessage],
        _cancel: CancellationToken,
    ) -> HandlerResult {
        HandlerResult::Reject {
            reason: "permanently bad tx".into(),
        }
    }
}

/// Rejects a specific message (by seq number), succeeds on others.
struct PoisonMessageHandler {
    poison_seqs: Vec<i64>,
}

#[async_trait::async_trait]
impl SingleHandler for PoisonMessageHandler {
    async fn handle(&self, msg: &OutboxMessage, _cancel: CancellationToken) -> HandlerResult {
        if self.poison_seqs.contains(&msg.seq) {
            HandlerResult::Reject {
                reason: format!("poison seq={}", msg.seq),
            }
        } else {
            HandlerResult::Success
        }
    }
}

// ======================================================================
// Chapter 1: Registration
// ======================================================================

#[tokio::test]
async fn registration_creates_partition_and_processor_rows() {
    let db = setup_db("ch1_creates_rows").await;
    let t = make_default_test_outbox();

    t.outbox.register_queue(&db, "orders", 4).await.unwrap();

    let part_count = count_rows(&db, "modkit_outbox_partitions").await;
    assert_eq!(part_count, 4, "4 partition rows");

    let proc_count = count_rows(&db, "modkit_outbox_processor").await;
    assert_eq!(proc_count, 4, "4 processor rows");

    // Each processor row starts at processed_seq=0, attempts=0
    let ids = t.outbox.all_partition_ids();
    for id in &ids {
        let snap = read_processor_state(&db, *id).await;
        assert_eq!(snap.processed_seq, 0);
        assert_eq!(snap.attempts, 0);
    }
}

#[tokio::test]
async fn registration_is_idempotent() {
    let db = setup_db("ch1_idempotent").await;
    let t = make_default_test_outbox();

    t.outbox.register_queue(&db, "orders", 4).await.unwrap();
    t.outbox.register_queue(&db, "orders", 4).await.unwrap();

    let part_count = count_rows(&db, "modkit_outbox_partitions").await;
    assert_eq!(part_count, 4, "still exactly 4 - no duplicates");
}

#[tokio::test]
async fn registration_rejects_mismatched_partition_count() {
    let db = setup_db("ch1_mismatch").await;
    let t = make_default_test_outbox();

    t.outbox.register_queue(&db, "orders", 4).await.unwrap();
    let err = t.outbox.register_queue(&db, "orders", 2).await.unwrap_err();

    assert!(matches!(
        err,
        OutboxError::PartitionCountMismatch {
            expected: 2,
            found: 4,
            ..
        }
    ));
}

#[tokio::test]
async fn registration_multiple_queues_distinct_ids() {
    let db = setup_db("ch1_multi_queue").await;
    let t = make_default_test_outbox();

    t.outbox.register_queue(&db, "a", 2).await.unwrap();
    t.outbox.register_queue(&db, "b", 2).await.unwrap();

    let all_ids = t.outbox.all_partition_ids();
    assert_eq!(all_ids.len(), 4);
    // All distinct (sorted + deduped by all_partition_ids)
    let mut deduped = all_ids;
    deduped.dedup();
    assert_eq!(deduped.len(), 4);
}

#[tokio::test]
async fn registration_partition_to_queue_reverse_lookup() {
    let db = setup_db("ch1_reverse_lookup").await;
    let t = make_default_test_outbox();

    t.outbox.register_queue(&db, "orders", 2).await.unwrap();

    let ids = t.outbox.all_partition_ids();
    assert_eq!(ids.len(), 2);
    for id in &ids {
        assert_eq!(t.outbox.partition_to_queue(*id).as_deref(), Some("orders"));
    }
}

// ======================================================================
// Chapter 2: Enqueue
// ======================================================================

#[tokio::test]
async fn enqueue_single_creates_body_and_incoming() {
    let db = setup_db("ch2_single").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    enqueue_msgs(&t.outbox, &db, "q", 0, &["hello"]).await;

    assert_eq!(count_rows(&db, "modkit_outbox_body").await, 1);
    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 1);
}

#[tokio::test]
async fn enqueue_returns_correct_id() {
    let db = setup_db("ch2_correct_id").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    let ids = enqueue_msgs(&t.outbox, &db, "q", 0, &["msg"]).await;
    assert_eq!(ids.len(), 1);
    // The returned ID should be the incoming row ID (positive integer)
    assert!(ids[0].0 > 0);
}

#[tokio::test]
async fn enqueue_tx_rollback_leaves_no_rows() {
    let db = setup_db("ch2_rollback").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    // Use sea_orm transaction directly to simulate rollback
    let conn = db.sea_internal();
    let txn = sea_orm::TransactionTrait::begin(&conn).await.unwrap();
    // Insert body + incoming manually through the transaction
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO modkit_outbox_body (payload, payload_type) VALUES ($1, $2)",
        [b"data".to_vec().into(), "text/plain".into()],
    ))
    .await
    .unwrap();
    // Rollback
    txn.rollback().await.unwrap();

    assert_eq!(count_rows(&db, "modkit_outbox_body").await, 0);
    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 0);
}

#[tokio::test]
async fn enqueue_with_standalone_connection() {
    let db = setup_db("ch2_standalone").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    // enqueue_msgs already uses db.conn() (standalone connection)
    enqueue_msgs(&t.outbox, &db, "q", 0, &["standalone"]).await;

    assert_eq!(count_rows(&db, "modkit_outbox_body").await, 1);
    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 1);
}

#[tokio::test]
async fn enqueue_batch_creates_n_items() {
    let db = setup_db("ch2_batch_n").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    let items: Vec<(u32, Vec<u8>, &str)> = (0..50)
        .map(|i| (0u32, format!("msg-{i}").into_bytes(), "text/plain"))
        .collect();
    let conn = db.conn().unwrap();
    let ids = t.outbox.enqueue_batch(&conn, "q", &items).await.unwrap();

    assert_eq!(ids.len(), 50);
    assert_eq!(count_rows(&db, "modkit_outbox_body").await, 50);
    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 50);
}

#[tokio::test]
async fn enqueue_batch_mixed_partitions() {
    let db = setup_db("ch2_batch_mixed").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 2).await.unwrap();

    let items: Vec<(u32, Vec<u8>, &str)> = vec![
        (0, b"a".to_vec(), "text/plain"),
        (1, b"b".to_vec(), "text/plain"),
        (0, b"c".to_vec(), "text/plain"),
        (1, b"d".to_vec(), "text/plain"),
    ];
    let conn = db.conn().unwrap();
    let ids = t.outbox.enqueue_batch(&conn, "q", &items).await.unwrap();
    assert_eq!(ids.len(), 4);
    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 4);
}

#[tokio::test]
async fn enqueue_batch_one_invalid_rejects_entire_batch() {
    let db = setup_db("ch2_batch_invalid").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    let oversized = vec![0u8; 64 * 1024 + 1];
    let items: Vec<(u32, Vec<u8>, &str)> = vec![
        (0, b"ok".to_vec(), "text/plain"),
        (0, oversized, "text/plain"),
    ];
    let conn = db.conn().unwrap();
    let err = t
        .outbox
        .enqueue_batch(&conn, "q", &items)
        .await
        .unwrap_err();
    assert!(matches!(err, OutboxError::PayloadTooLarge { .. }));
    assert_eq!(count_rows(&db, "modkit_outbox_body").await, 0);
}

#[tokio::test]
async fn enqueue_empty_batch_returns_empty_vec() {
    let db = setup_db("ch2_batch_empty").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    let conn = db.conn().unwrap();
    let ids = t.outbox.enqueue_batch(&conn, "q", &[]).await.unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn enqueue_batch_over_chunk_size_works() {
    let db = setup_db("ch2_batch_chunk").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    let items: Vec<(u32, Vec<u8>, &str)> = (0..150)
        .map(|i| (0u32, format!("msg-{i}").into_bytes(), "text/plain"))
        .collect();
    let conn = db.conn().unwrap();
    let ids = t.outbox.enqueue_batch(&conn, "q", &items).await.unwrap();

    assert_eq!(ids.len(), 150);
    assert_eq!(count_rows(&db, "modkit_outbox_body").await, 150);
    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 150);
}

#[tokio::test]
async fn enqueue_oversized_payload_rejected() {
    let db = setup_db("ch2_oversized").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    let oversized = vec![0u8; 64 * 1024 + 1];
    let conn = db.conn().unwrap();
    let err = t
        .outbox
        .enqueue(&conn, "q", 0, oversized, "bin")
        .await
        .unwrap_err();
    assert!(matches!(err, OutboxError::PayloadTooLarge { .. }));
}

#[tokio::test]
async fn enqueue_unregistered_queue_rejected() {
    let db = setup_db("ch2_unreg").await;
    let t = make_default_test_outbox();
    // Don't register any queue

    let conn = db.conn().unwrap();
    let err = t
        .outbox
        .enqueue(&conn, "nonexistent", 0, b"x".to_vec(), "text/plain")
        .await
        .unwrap_err();
    assert!(matches!(err, OutboxError::QueueNotRegistered(_)));
}

#[tokio::test]
async fn enqueue_out_of_range_partition_rejected() {
    let db = setup_db("ch2_oor").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 4).await.unwrap();

    let conn = db.conn().unwrap();
    let err = t
        .outbox
        .enqueue(&conn, "q", 5, b"x".to_vec(), "text/plain")
        .await
        .unwrap_err();
    assert!(matches!(err, OutboxError::PartitionOutOfRange { .. }));
}

#[tokio::test]
async fn enqueue_transaction_helper_auto_flushes() {
    let db = setup_db("ch2_tx_flush").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    // Set up a notified() listener before the transaction
    let notified = t.sequencer_notify.notified();

    let (_db, result) = t
        .outbox
        .transaction(db, |tx| {
            let outbox = Arc::clone(&t.outbox);
            Box::pin(async move {
                outbox
                    .enqueue(tx, "q", 0, b"hello".to_vec(), "text/plain")
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(())
            })
        })
        .await;
    result.unwrap();

    // Notify should fire within a short timeout
    tokio::time::timeout(Duration::from_millis(100), notified)
        .await
        .expect("sequencer should be notified on successful transaction");
}

#[tokio::test]
async fn enqueue_transaction_helper_no_flush_on_rollback() {
    let db = setup_db("ch2_tx_no_flush").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    let (_db, result) = t
        .outbox
        .transaction(db, |_tx| {
            Box::pin(async move { Err::<(), _>(anyhow::anyhow!("rollback")) })
        })
        .await;
    assert!(result.is_err());

    // Give a brief window — notify should NOT fire
    let notified = t.sequencer_notify.notified();
    let timed_out = tokio::time::timeout(Duration::from_millis(50), notified)
        .await
        .is_err();
    assert!(timed_out, "sequencer should NOT be notified on rollback");
}

// ======================================================================
// Chapter 3: Sequencer
// ======================================================================

#[tokio::test]
async fn sequencer_moves_incoming_to_outgoing() {
    let db = setup_db("ch3_moves").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    enqueue_msgs(&t.outbox, &db, "q", 0, &["a", "b", "c"]).await;
    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 3);

    run_sequencer_once(&t, &db).await;

    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 0);
    assert_eq!(count_rows(&db, "modkit_outbox_outgoing").await, 3);

    let pid = t.outbox.all_partition_ids()[0];
    let outgoing = read_outgoing(&db, pid).await;
    let seqs: Vec<i64> = outgoing.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3]);

    // Verify structural fields: each row belongs to the queried partition,
    // has a positive id, and references a valid body row.
    for row in &outgoing {
        assert_eq!(row.partition_id, pid);
        assert!(row.id > 0);
        assert!(row.body_id > 0);
    }
    // IDs should be unique
    let ids: Vec<i64> = outgoing.iter().map(|r| r.id).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids[0] != ids[1] && ids[1] != ids[2]);
}

/// Enqueue many messages to one partition, sequence them, and verify the
/// outgoing sequence order matches the original enqueue (insertion) order.
/// This guards against non-deterministic row ordering in the claim step.
#[tokio::test]
async fn sequencer_preserves_enqueue_order_in_sequences() {
    let db = setup_db("ch3_fifo").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    // Enqueue 8 messages — enough to surface ordering issues
    let payloads: Vec<String> = (0..8).map(|i| format!("msg-{i}")).collect();
    let payload_refs: Vec<&str> = payloads.iter().map(String::as_str).collect();
    let enqueue_ids = enqueue_msgs(&t.outbox, &db, "q", 0, &payload_refs).await;

    run_sequencer_once(&t, &db).await;

    let pid = t.outbox.all_partition_ids()[0];
    let outgoing = read_outgoing(&db, pid).await;

    // Sequences must be strictly monotonically increasing
    let seqs: Vec<i64> = outgoing.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5, 6, 7, 8]);

    // body_ids must follow the same order as enqueue_ids (insertion order)
    let body_ids: Vec<i64> = outgoing.iter().map(|r| r.body_id).collect();
    for i in 1..body_ids.len() {
        assert!(
            body_ids[i] > body_ids[i - 1],
            "body_id[{i}]={} should be > body_id[{}]={}",
            body_ids[i],
            i - 1,
            body_ids[i - 1]
        );
    }

    // Verify count matches
    assert_eq!(enqueue_ids.len(), 8);
    assert_eq!(outgoing.len(), 8);
}

#[tokio::test]
async fn sequencer_updates_partition_sequence_counter() {
    let db = setup_db("ch3_seq_counter").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    enqueue_msgs(&t.outbox, &db, "q", 0, &["a", "b", "c"]).await;
    run_sequencer_once(&t, &db).await;

    let pid = t.outbox.all_partition_ids()[0];
    let seq = read_partition_sequence(&db, pid).await;
    assert_eq!(seq, 3);
}

#[tokio::test]
async fn sequencer_multi_partition_independent_sequences() {
    let db = setup_db("ch3_multi_part").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 2).await.unwrap();

    enqueue_msgs(&t.outbox, &db, "q", 0, &["a0", "b0"]).await;
    enqueue_msgs(&t.outbox, &db, "q", 1, &["a1", "b1", "c1"]).await;
    run_sequencer_once(&t, &db).await;

    let ids = t.outbox.all_partition_ids();
    let out0 = read_outgoing(&db, ids[0]).await;
    let out1 = read_outgoing(&db, ids[1]).await;

    let seqs0: Vec<i64> = out0.iter().map(|r| r.seq).collect();
    let seqs1: Vec<i64> = out1.iter().map(|r| r.seq).collect();
    assert_eq!(seqs0, vec![1, 2]);
    assert_eq!(seqs1, vec![1, 2, 3]);
}

#[tokio::test]
async fn sequencer_empty_incoming_returns_zero() {
    let db = setup_db("ch3_empty").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    let seq = make_sequencer(&t, SequencerConfig::default());
    let result = seq.sequence_batch(&db).await.unwrap();
    assert_eq!(result.total, 0);
    assert!(!result.any_partition_saturated);
}

#[tokio::test]
async fn sequencer_consecutive_batches_contiguous_sequences() {
    let db = setup_db("ch3_contiguous").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    enqueue_msgs(&t.outbox, &db, "q", 0, &["a", "b"]).await;
    run_sequencer_once(&t, &db).await;

    enqueue_msgs(&t.outbox, &db, "q", 0, &["c", "d"]).await;
    run_sequencer_once(&t, &db).await;

    let pid = t.outbox.all_partition_ids()[0];
    let outgoing = read_outgoing(&db, pid).await;
    let seqs: Vec<i64> = outgoing.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4]);
}

#[tokio::test]
async fn sequencer_batch_size_limit_enforced() {
    let db = setup_db("ch3_batch_limit").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    enqueue_msgs(&t.outbox, &db, "q", 0, &["a", "b", "c", "d", "e"]).await;

    let seq = make_sequencer(
        &t,
        SequencerConfig {
            batch_size: 2,
            ..Default::default()
        },
    );
    let result = seq.sequence_batch(&db).await.unwrap();
    assert_eq!(result.total, 2, "only 2 claimed per batch");
    assert!(result.any_partition_saturated);
}

#[tokio::test]
async fn sequencer_saturated_partition_sets_has_more() {
    let db = setup_db("ch3_saturated").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    enqueue_msgs(&t.outbox, &db, "q", 0, &["a", "b", "c"]).await;

    let seq = make_sequencer(
        &t,
        SequencerConfig {
            batch_size: 2,
            ..Default::default()
        },
    );
    let result = seq.sequence_batch(&db).await.unwrap();
    assert!(result.any_partition_saturated);
}

#[tokio::test]
async fn sequencer_unsaturated_partitions_clear_has_more() {
    let db = setup_db("ch3_unsaturated").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    enqueue_msgs(&t.outbox, &db, "q", 0, &["a"]).await;

    let seq = make_sequencer(
        &t,
        SequencerConfig {
            batch_size: 100,
            ..Default::default()
        },
    );
    let result = seq.sequence_batch(&db).await.unwrap();
    assert!(!result.any_partition_saturated);
}

#[tokio::test]
async fn sequencer_skips_empty_partitions() {
    let db = setup_db("ch3_skip_empty").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 2).await.unwrap();

    // Only enqueue to partition 1, not partition 0
    enqueue_msgs(&t.outbox, &db, "q", 1, &["only-p1"]).await;
    run_sequencer_once(&t, &db).await;

    let ids = t.outbox.all_partition_ids();
    let out0 = read_outgoing(&db, ids[0]).await;
    let out1 = read_outgoing(&db, ids[1]).await;

    assert!(out0.is_empty(), "partition 0 should have no outgoing");
    assert_eq!(out1.len(), 1, "partition 1 should have 1 outgoing");
}

// ======================================================================
// Chapter 4: Transactional Processing
// ======================================================================

async fn run_transactional(
    db: &Db,
    partition_id: i64,
    handler: impl TransactionalHandler + 'static,
    config: &QueueConfig,
) -> Option<super::strategy::ProcessResult> {
    let conn = db.sea_internal();
    let backend = conn.get_database_backend();
    let dialect = Dialect::from(backend);
    drop(conn);

    let strategy = TransactionalStrategy::new(Box::new(handler));
    let ctx = ProcessContext {
        db,
        backend,
        dialect,
        partition_id,
    };
    strategy
        .process(&ctx, config, CancellationToken::new())
        .await
        .unwrap()
}

#[tokio::test]
async fn transactional_success_advances_cursor() {
    let db = setup_db("ch4_tx_success").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["a", "b", "c"]).await;

    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig {
        msg_batch_size: 3,
        ..Default::default()
    };
    run_transactional(
        &db,
        pid,
        CountingTxHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    assert_eq!(count.load(Ordering::Relaxed), 3);
    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 3);
    assert_eq!(snap.attempts, 0);
}

#[tokio::test]
async fn transactional_retry_increments_attempts() {
    let db = setup_db("ch4_tx_retry").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["msg"]).await;

    let config = QueueConfig::default();
    run_transactional(&db, pid, AlwaysRetryTxHandler, &config).await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 0, "cursor not advanced");
    assert_eq!(snap.attempts, 1);
    assert_eq!(snap.last_error.as_deref(), Some("transient tx failure"));
}

#[tokio::test]
async fn transactional_reject_creates_dead_letter_and_advances() {
    let db = setup_db("ch4_tx_reject").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["poison"]).await;

    let config = QueueConfig::default();
    run_transactional(&db, pid, AlwaysRejectTxHandler, &config).await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 1, "cursor advanced past rejected msg");

    let dls = read_dead_letters(&db).await;
    assert_eq!(dls.len(), 1);
    assert!(dls[0].id > 0);
    assert_eq!(dls[0].partition_id, pid);
    assert_eq!(dls[0].seq, 1);
    assert_eq!(dls[0].last_error.as_deref(), Some("permanently bad tx"));
    assert_eq!(dls[0].payload, b"poison");
    assert_eq!(dls[0].payload_type, "text/plain");
    assert_eq!(dls[0].attempts, 0);
    assert!(dls[0].replayed_at.is_none());
}

#[tokio::test]
async fn transactional_batch_processes_multiple_in_single_tx() {
    let db = setup_db("ch4_tx_batch").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["a", "b", "c"]).await;

    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig {
        msg_batch_size: 3,
        ..Default::default()
    };
    // CountingTxHandler counts the number of messages per call
    run_transactional(
        &db,
        pid,
        CountingTxHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    // Handler called once with all 3 messages
    assert_eq!(count.load(Ordering::Relaxed), 3);
}

// ======================================================================
// Chapter 5: Decoupled Processing
// ======================================================================

async fn run_decoupled(
    db: &Db,
    partition_id: i64,
    handler: impl Handler + 'static,
    config: &QueueConfig,
) -> Option<super::strategy::ProcessResult> {
    let conn = db.sea_internal();
    let backend = conn.get_database_backend();
    let dialect = Dialect::from(backend);
    drop(conn);

    let strategy = DecoupledStrategy::new(Box::new(handler));
    let ctx = ProcessContext {
        db,
        backend,
        dialect,
        partition_id,
    };
    strategy
        .process(&ctx, config, CancellationToken::new())
        .await
        .unwrap()
}

#[tokio::test]
async fn decoupled_success_advances_cursor_and_releases_lease() {
    let db = setup_db("ch5_dc_success").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["a", "b"]).await;

    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig {
        msg_batch_size: 2,
        ..Default::default()
    };
    run_decoupled(
        &db,
        pid,
        CountingSuccessHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    assert_eq!(count.load(Ordering::Relaxed), 2);
    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 2);
    assert_eq!(snap.attempts, 0);
    assert!(snap.locked_by.is_none(), "lease released");
    assert!(snap.locked_until.is_none(), "lease released");
}

#[tokio::test]
async fn decoupled_retry_preserves_cursor_and_releases_lease() {
    let db = setup_db("ch5_dc_retry").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["msg"]).await;

    let config = QueueConfig::default();
    run_decoupled(&db, pid, EachMessage(AlwaysRetryHandler), &config).await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 0, "cursor unchanged");
    assert_eq!(snap.attempts, 1, "attempts incremented by lease_acquire");
    assert_eq!(snap.last_error.as_deref(), Some("transient failure"));
    assert!(snap.locked_by.is_none(), "lease released");
}

#[tokio::test]
async fn decoupled_reject_creates_dead_letter_and_releases_lease() {
    let db = setup_db("ch5_dc_reject").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["bad"]).await;

    let config = QueueConfig::default();
    run_decoupled(&db, pid, EachMessage(AlwaysRejectHandler), &config).await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 1, "cursor advanced past rejected");
    assert!(snap.locked_by.is_none(), "lease released");

    let dls = read_dead_letters(&db).await;
    assert_eq!(dls.len(), 1);
    assert_eq!(dls[0].last_error.as_deref(), Some("permanently bad"));
}

#[tokio::test]
async fn decoupled_empty_partition_releases_lease() {
    let db = setup_db("ch5_dc_empty").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    // No messages enqueued
    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig::default();
    let result = run_decoupled(
        &db,
        pid,
        CountingSuccessHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    assert!(result.is_none(), "no work done");
    assert_eq!(count.load(Ordering::Relaxed), 0);
    let snap = read_processor_state(&db, pid).await;
    assert!(snap.locked_by.is_none(), "lease released after empty");
}

#[tokio::test]
async fn decoupled_each_message_adapter_processes_individually() {
    let db = setup_db("ch5_dc_each").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["a", "b", "c"]).await;

    let count = Arc::new(AtomicU32::new(0));
    let handler = EachMessage(CountingSingleHandler {
        count: count.clone(),
    });
    let config = QueueConfig {
        msg_batch_size: 3,
        ..Default::default()
    };
    run_decoupled(&db, pid, handler, &config).await;

    // EachMessage calls SingleHandler once per message
    assert_eq!(count.load(Ordering::Relaxed), 3);
}

// ======================================================================
// Chapter 6: Crash Detection & Recovery
// ======================================================================

#[tokio::test]
async fn crash_leaves_incremented_attempts_in_db() {
    let db = setup_db("ch6_crash_trace").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["msg"]).await;

    // Simulate: lease acquired (attempts incremented in DB), then pod dies
    simulate_crash(&db, pid, 300).await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.attempts, 1, "crash left incremented attempts");
    assert_eq!(snap.processed_seq, 0, "cursor unchanged");
    assert!(snap.locked_by.is_some(), "lease still held by crashed pod");
}

#[tokio::test]
async fn recovery_after_crash_sees_nonzero_attempts() {
    let db = setup_db("ch6_recovery").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["msg"]).await;

    // Crash + expire lease so a new processor can acquire it
    simulate_crash(&db, pid, 300).await;
    expire_lease(&db, pid).await;

    // Recovery processor should see attempts=1 (from the crash)
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler = AttemptsRecorder {
        seen_attempts: seen.clone(),
    };
    let config = QueueConfig::default();
    run_decoupled(&db, pid, EachMessage(handler), &config).await;

    {
        let recorded = seen.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], 1, "handler sees attempts=1 from the crash");
    }

    // After success, attempts reset to 0
    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.attempts, 0);
}

#[tokio::test]
async fn multiple_crashes_accumulate_attempts() {
    let db = setup_db("ch6_multi_crash").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["msg"]).await;

    // Two crashes
    simulate_crash(&db, pid, 300).await;
    expire_lease(&db, pid).await;
    simulate_crash(&db, pid, 300).await;
    expire_lease(&db, pid).await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.attempts, 2, "two crashes accumulated");
}

#[tokio::test]
async fn retry_does_not_double_increment_attempts() {
    let db = setup_db("ch6_no_double").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["msg"]).await;

    // lease_acquire increments attempts 0→1 in DB;
    // handler returns Retry; lease_record_retry does NOT increment again
    let config = QueueConfig::default();
    run_decoupled(&db, pid, EachMessage(AlwaysRetryHandler), &config).await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.attempts, 1, "not 2 - retry doesn't double-increment");
}

#[tokio::test]
async fn success_after_crash_resets_attempts() {
    let db = setup_db("ch6_reset").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["msg"]).await;

    // Crash
    simulate_crash(&db, pid, 300).await;
    expire_lease(&db, pid).await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.attempts, 1);

    // Recovery succeeds
    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig::default();
    run_decoupled(
        &db,
        pid,
        CountingSuccessHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.attempts, 0, "success resets attempts to 0");
    assert_eq!(snap.processed_seq, 1);
}

// ======================================================================
// Chapter 7: Backoff & Adaptive Batching
// ======================================================================

#[tokio::test]
async fn adaptive_batch_isolates_poison_message() {
    let db = setup_db("ch7_poison").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    // Enqueue 4 messages; message at seq=2 is the poison pill
    enqueue_and_sequence(&t, &db, "q", 0, &["ok1", "poison", "ok3", "ok4"]).await;

    // Demonstrate the adaptive batch isolation mechanism step by step
    // with batch_size=1 (the degraded size):

    let config = QueueConfig {
        msg_batch_size: 1,
        ..Default::default()
    };

    // Process msg 1 (ok1) — success
    let r = run_decoupled(
        &db,
        pid,
        EachMessage(PoisonMessageHandler {
            poison_seqs: vec![2],
        }),
        &config,
    )
    .await;
    assert!(matches!(r.unwrap().handler_result, HandlerResult::Success));

    // Process msg 2 (poison) — reject, dead-lettered
    let r = run_decoupled(
        &db,
        pid,
        EachMessage(PoisonMessageHandler {
            poison_seqs: vec![2],
        }),
        &config,
    )
    .await;
    assert!(matches!(
        r.unwrap().handler_result,
        HandlerResult::Reject { .. }
    ));

    // Process msg 3 (ok3) — success
    let r = run_decoupled(
        &db,
        pid,
        EachMessage(PoisonMessageHandler {
            poison_seqs: vec![2],
        }),
        &config,
    )
    .await;
    assert!(matches!(r.unwrap().handler_result, HandlerResult::Success));

    // Process msg 4 (ok4) — success
    let r = run_decoupled(
        &db,
        pid,
        EachMessage(PoisonMessageHandler {
            poison_seqs: vec![2],
        }),
        &config,
    )
    .await;
    assert!(matches!(r.unwrap().handler_result, HandlerResult::Success));

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 4, "all 4 messages processed");

    let dls = read_dead_letters(&db).await;
    assert_eq!(dls.len(), 1, "only the poison message was dead-lettered");
    assert_eq!(dls[0].seq, 2);
}

// ======================================================================
// Chapter 8: Reaper
// ======================================================================

/// Run the reaper by processing an empty partition (reaper triggers on idle).
async fn run_reaper(db: &Db, partition_id: i64) {
    #[derive(Debug, FromQueryResult)]
    struct ProcRow {
        processed_seq: i64,
    }

    let conn = db.sea_internal();
    let backend = conn.get_database_backend();
    let dialect = Dialect::from(backend);

    let proc_row = ProcRow::find_by_statement(Statement::from_sql_and_values(
        backend,
        "SELECT processed_seq FROM modkit_outbox_processor WHERE partition_id = $1",
        [partition_id.into()],
    ))
    .one(&conn)
    .await
    .unwrap()
    .unwrap();

    if proc_row.processed_seq == 0 {
        return;
    }

    let txn = conn.begin().await.unwrap();
    match dialect.reaper_cleanup() {
        super::dialect::ReaperSql::Cte(sql) => {
            txn.execute(Statement::from_sql_and_values(
                backend,
                sql,
                [partition_id.into(), proc_row.processed_seq.into()],
            ))
            .await
            .unwrap();
        }
        super::dialect::ReaperSql::TwoStep {
            select_body_ids,
            delete_outgoing,
        } => {
            let rows = txn
                .query_all(Statement::from_sql_and_values(
                    backend,
                    select_body_ids,
                    [partition_id.into(), proc_row.processed_seq.into()],
                ))
                .await
                .unwrap();
            let body_ids: Vec<i64> = rows
                .iter()
                .filter_map(|r| r.try_get_by_index::<i64>(0).ok())
                .collect();
            txn.execute(Statement::from_sql_and_values(
                backend,
                delete_outgoing,
                [partition_id.into(), proc_row.processed_seq.into()],
            ))
            .await
            .unwrap();
            for body_id in body_ids {
                txn.execute(Statement::from_sql_and_values(
                    backend,
                    dialect.delete_body(),
                    [body_id.into()],
                ))
                .await
                .unwrap();
            }
        }
    }
    txn.commit().await.unwrap();
}

#[tokio::test]
async fn reaper_deletes_processed_outgoing_and_body_rows() {
    let db = setup_db("ch8_reaper_deletes").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["a", "b", "c"]).await;

    // Process all 3
    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig {
        msg_batch_size: 3,
        ..Default::default()
    };
    run_decoupled(
        &db,
        pid,
        CountingSuccessHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    // Reap
    run_reaper(&db, pid).await;

    assert_eq!(count_rows(&db, "modkit_outbox_outgoing").await, 0);
    assert_eq!(count_rows(&db, "modkit_outbox_body").await, 0);

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 3, "cursor preserved");
}

#[tokio::test]
async fn reaper_skips_when_processed_seq_is_zero() {
    let db = setup_db("ch8_reaper_skip").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["a"]).await;

    // Don't process — cursor at 0
    run_reaper(&db, pid).await;

    assert_eq!(
        count_rows(&db, "modkit_outbox_outgoing").await,
        1,
        "rows preserved"
    );
}

#[tokio::test]
async fn reaper_preserves_unprocessed_rows() {
    let db = setup_db("ch8_reaper_preserves").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["a", "b", "c", "d", "e"]).await;

    // Process only 3 of 5 (msg_batch_size=3)
    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig {
        msg_batch_size: 3,
        ..Default::default()
    };
    run_decoupled(
        &db,
        pid,
        CountingSuccessHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 3);

    // Reap — should only delete seqs 1-3
    run_reaper(&db, pid).await;

    let remaining = read_outgoing(&db, pid).await;
    assert_eq!(remaining.len(), 2);
    let seqs: Vec<i64> = remaining.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![4, 5]);
    for row in &remaining {
        assert_eq!(row.partition_id, pid);
        assert!(row.id > 0);
        assert!(row.body_id > 0);
    }
}

// ======================================================================
// Chapter 9: Dead Letters
// ======================================================================

/// Helper: enqueue, sequence, and reject N messages to create dead letters.
async fn create_dead_letters(
    t: &TestOutbox,
    db: &Db,
    queue: &str,
    partition: u32,
    payloads: &[&str],
) {
    enqueue_and_sequence(t, db, queue, partition, payloads).await;
    let ids = t.outbox.all_partition_ids();
    let pid = ids[partition as usize];
    let config = QueueConfig {
        msg_batch_size: u32::try_from(payloads.len()).unwrap(),
        ..Default::default()
    };
    run_decoupled(db, pid, EachMessage(AlwaysRejectHandler), &config).await;
}

#[tokio::test]
async fn dead_letter_list_returns_correct_fields() {
    let db = setup_db("ch9_dl_list").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    create_dead_letters(&t, &db, "q", 0, &["a", "b", "c"]).await;

    let items = DeadLetterOps::list(&db, &DeadLetterFilter::default())
        .await
        .unwrap();
    assert_eq!(items.len(), 3);
    for item in &items {
        assert_eq!(item.partition_id, pid);
        assert_eq!(item.last_error.as_deref(), Some("permanently bad"));
        assert!(item.replayed_at.is_none());
    }
}

#[tokio::test]
async fn dead_letter_count_matches_list() {
    let db = setup_db("ch9_dl_count").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    create_dead_letters(&t, &db, "q", 0, &["a", "b", "c"]).await;

    let count = DeadLetterOps::count(&db, &DeadLetterFilter::default())
        .await
        .unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn dead_letter_replay_reinserts_and_sets_replayed_at() {
    let db = setup_db("ch9_dl_replay").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    create_dead_letters(&t, &db, "q", 0, &["msg"]).await;

    let replayed = DeadLetterOps::replay(&db, &DeadLetterFilter::default())
        .await
        .unwrap();
    assert_eq!(replayed, 1);

    // New incoming row created
    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 1);

    // Dead letter now has replayed_at set
    let dls = read_dead_letters(&db).await;
    assert_eq!(dls.len(), 1);
    assert!(dls[0].replayed_at.is_some());
}

#[tokio::test]
async fn dead_letter_full_replay_roundtrip() {
    let db = setup_db("ch9_dl_roundtrip").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    // Reject
    create_dead_letters(&t, &db, "q", 0, &["msg"]).await;

    // Replay → re-sequence → process with success handler
    DeadLetterOps::replay(&db, &DeadLetterFilter::default())
        .await
        .unwrap();
    run_sequencer_once(&t, &db).await;

    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig::default();
    run_decoupled(
        &db,
        pid,
        CountingSuccessHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    assert_eq!(count.load(Ordering::Relaxed), 1);
    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 2); // seq 1 was rejected, seq 2 is the replay
}

#[tokio::test]
async fn dead_letter_purge_non_force_only_replayed() {
    let db = setup_db("ch9_dl_purge_soft").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    // Create 2 dead letters
    create_dead_letters(&t, &db, "q", 0, &["a", "b"]).await;

    // Replay only 1 (by limit)
    let filter_one = DeadLetterFilter {
        limit: Some(1),
        ..Default::default()
    };
    DeadLetterOps::replay(&db, &filter_one).await.unwrap();

    // Purge non-force — should only delete the replayed one
    let deleted = DeadLetterOps::purge(
        &db,
        &DeadLetterFilter {
            only_pending: false,
            ..Default::default()
        },
        false,
    )
    .await
    .unwrap();
    assert_eq!(deleted, 1);

    // 1 pending dead letter remains
    let remaining = DeadLetterOps::count(&db, &DeadLetterFilter::default())
        .await
        .unwrap();
    assert_eq!(remaining, 1);
}

#[tokio::test]
async fn dead_letter_purge_force_deletes_all() {
    let db = setup_db("ch9_dl_purge_force").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    create_dead_letters(&t, &db, "q", 0, &["a", "b", "c"]).await;

    let deleted = DeadLetterOps::purge(
        &db,
        &DeadLetterFilter {
            only_pending: false,
            ..Default::default()
        },
        true,
    )
    .await
    .unwrap();
    assert_eq!(deleted, 3);

    let remaining = DeadLetterOps::count(
        &db,
        &DeadLetterFilter {
            only_pending: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn dead_letter_filter_by_partition() {
    let db = setup_db("ch9_dl_filter_part").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 2).await.unwrap();
    let ids = t.outbox.all_partition_ids();

    // Dead-letter messages on both partitions
    create_dead_letters(&t, &db, "q", 0, &["a0"]).await;
    create_dead_letters(&t, &db, "q", 1, &["b1", "b2"]).await;

    let filter_p0 = DeadLetterFilter {
        partition_id: Some(ids[0]),
        ..Default::default()
    };
    let items = DeadLetterOps::list(&db, &filter_p0).await.unwrap();
    assert_eq!(items.len(), 1);

    let filter_p1 = DeadLetterFilter {
        partition_id: Some(ids[1]),
        ..Default::default()
    };
    let items = DeadLetterOps::list(&db, &filter_p1).await.unwrap();
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn dead_letter_filter_with_limit() {
    let db = setup_db("ch9_dl_filter_limit").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();

    create_dead_letters(&t, &db, "q", 0, &["a", "b", "c", "d", "e"]).await;

    let filter = DeadLetterFilter {
        limit: Some(2),
        ..Default::default()
    };
    let items = DeadLetterOps::list(&db, &filter).await.unwrap();
    assert_eq!(items.len(), 2);
}

// ======================================================================
// Chapter 10: Builder API
// ======================================================================

#[tokio::test]
async fn builder_start_stop_clean() {
    let db = setup_db("ch10_start_stop").await;

    let count = Arc::new(AtomicU32::new(0));
    let handler = CountingSingleHandler {
        count: count.clone(),
    };
    let handle = Outbox::builder(db)
        .poll_interval(Duration::from_millis(50))
        .queue("orders", Partitions::of(1))
        .decoupled(handler)
        .start()
        .await
        .unwrap();

    // Just verify it started and can stop cleanly
    handle.stop().await;
}

#[tokio::test]
async fn builder_partitions_of_all_valid_values() {
    for n in [1, 2, 4, 8, 16, 32, 64] {
        let p = Partitions::of(n);
        assert_eq!(p.count(), n);
    }
}

#[tokio::test]
async fn builder_multiple_queues() {
    let db = setup_db("ch10_multi_queue").await;

    let count_a = Arc::new(AtomicU32::new(0));
    let count_b = Arc::new(AtomicU32::new(0));

    let handle = Outbox::builder(db)
        .poll_interval(Duration::from_millis(50))
        .queue("a", Partitions::of(1))
        .decoupled(CountingSingleHandler {
            count: count_a.clone(),
        })
        .queue("b", Partitions::of(2))
        .decoupled(CountingSingleHandler {
            count: count_b.clone(),
        })
        .start()
        .await
        .unwrap();

    let outbox = handle.outbox();

    // Enqueue to both queues via a shared-cache connection
    let db2 = setup_db("ch10_multi_queue").await;
    let conn = db2.conn().unwrap();
    outbox
        .enqueue(&conn, "a", 0, b"hello-a".to_vec(), "text/plain")
        .await
        .unwrap();
    outbox
        .enqueue(&conn, "b", 0, b"hello-b".to_vec(), "text/plain")
        .await
        .unwrap();
    outbox.flush();

    // Wait for processing
    poll_until(
        || {
            let ca = count_a.load(Ordering::Relaxed);
            let cb = count_b.load(Ordering::Relaxed);
            async move { ca >= 1 && cb >= 1 }
        },
        5000,
    )
    .await;

    assert!(count_a.load(Ordering::Relaxed) >= 1);
    assert!(count_b.load(Ordering::Relaxed) >= 1);

    handle.stop().await;
}

// ======================================================================
// Chapter 11: End-to-End Lifecycle
// ======================================================================

#[tokio::test]
async fn e2e_happy_path_enqueue_through_reap() {
    let db = setup_db("ch11_happy").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    // Enqueue → Sequence → Process (decoupled, success) → Reap
    enqueue_and_sequence(&t, &db, "q", 0, &["a", "b", "c"]).await;

    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig {
        msg_batch_size: 3,
        ..Default::default()
    };
    run_decoupled(
        &db,
        pid,
        CountingSuccessHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;
    run_reaper(&db, pid).await;

    assert_eq!(count_rows(&db, "modkit_outbox_incoming").await, 0);
    assert_eq!(count_rows(&db, "modkit_outbox_outgoing").await, 0);
    assert_eq!(count_rows(&db, "modkit_outbox_body").await, 0);
    assert_eq!(count_rows(&db, "modkit_outbox_dead_letters").await, 0);

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 3);
    assert_eq!(snap.attempts, 0);
}

#[tokio::test]
async fn e2e_retry_then_recovery() {
    let db = setup_db("ch11_retry").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["msg"]).await;

    let config = QueueConfig::default();

    // Retry twice
    run_decoupled(&db, pid, EachMessage(AlwaysRetryHandler), &config).await;
    expire_lease(&db, pid).await;
    run_decoupled(&db, pid, EachMessage(AlwaysRetryHandler), &config).await;
    expire_lease(&db, pid).await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 0);
    assert_eq!(snap.attempts, 2);

    // Then succeed
    let count = Arc::new(AtomicU32::new(0));
    run_decoupled(
        &db,
        pid,
        CountingSuccessHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 1);
    assert_eq!(snap.attempts, 0, "attempts reset on success");
}

#[tokio::test]
async fn e2e_reject_replay_success() {
    let db = setup_db("ch11_reject_replay").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    // Reject
    create_dead_letters(&t, &db, "q", 0, &["msg"]).await;

    // Replay → re-sequence → process with success
    DeadLetterOps::replay(&db, &DeadLetterFilter::default())
        .await
        .unwrap();
    run_sequencer_once(&t, &db).await;

    let count = Arc::new(AtomicU32::new(0));
    let config = QueueConfig::default();
    run_decoupled(
        &db,
        pid,
        CountingSuccessHandler {
            count: count.clone(),
        },
        &config,
    )
    .await;

    // Reap
    run_reaper(&db, pid).await;

    assert_eq!(count.load(Ordering::Relaxed), 1);
    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 2);

    // Dead letter has replayed_at set
    let dls = read_dead_letters(&db).await;
    assert_eq!(dls.len(), 1);
    assert!(dls[0].replayed_at.is_some());

    // Body and outgoing cleaned up
    assert_eq!(count_rows(&db, "modkit_outbox_outgoing").await, 0);
    assert_eq!(count_rows(&db, "modkit_outbox_body").await, 0);
}

#[tokio::test]
async fn e2e_crash_then_recovery() {
    let db = setup_db("ch11_crash").await;
    let t = make_default_test_outbox();
    t.outbox.register_queue(&db, "q", 1).await.unwrap();
    let pid = t.outbox.all_partition_ids()[0];

    enqueue_and_sequence(&t, &db, "q", 0, &["msg"]).await;

    // Simulate crash
    simulate_crash(&db, pid, 300).await;
    expire_lease(&db, pid).await;

    // Recovery processor succeeds
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler = AttemptsRecorder {
        seen_attempts: seen.clone(),
    };
    let config = QueueConfig::default();
    run_decoupled(&db, pid, EachMessage(handler), &config).await;

    {
        let recorded = seen.lock().unwrap();
        assert_eq!(recorded[0], 1, "handler saw attempts=1 from crash");
    }

    let snap = read_processor_state(&db, pid).await;
    assert_eq!(snap.processed_seq, 1);
    assert_eq!(snap.attempts, 0, "attempts reset after successful recovery");
}
