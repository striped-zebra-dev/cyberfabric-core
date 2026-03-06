use std::sync::Arc;

use dashmap::DashMap;
use sea_orm::{ConnectionTrait, DbBackend, FromQueryResult, Statement};
use tokio::sync::{Notify, RwLock};

use super::dialect::Dialect;
use super::manager::OutboxBuilder;
use super::types::{OutboxConfig, OutboxError, OutboxItemId};
use crate::Db;
use crate::secure::SeaOrmRunner;

/// Core outbox handle. Holds partition cache and notification channels.
pub struct Outbox {
    config: OutboxConfig,
    /// Cached partition lookup: `partitions[queue_name][partition_number] = partitions.id` (PK).
    partitions: DashMap<String, Vec<i64>>,
    /// Reverse map: `partition_id → queue_name`. Populated during `register_queue`.
    partition_to_queue: DashMap<i64, String>,
    /// Flattened, sorted, deduplicated snapshot of all partition IDs.
    /// Rebuilt on each `register_queue` call.
    all_partition_ids: RwLock<Vec<i64>>,
    sequencer_notify: Arc<Notify>,
}

#[derive(Debug, FromQueryResult)]
struct PartitionRow {
    id: i64,
}

impl Outbox {
    /// Create a fluent builder for the outbox pipeline.
    ///
    /// This is the main entry point. See [`OutboxBuilder`] for usage.
    #[must_use]
    pub fn builder(db: Db) -> OutboxBuilder {
        OutboxBuilder::new(db)
    }

    /// Create a new outbox. Construction goes through [`OutboxBuilder::start()`].
    #[must_use]
    pub(crate) fn new(config: OutboxConfig, sequencer_notify: Arc<Notify>) -> Self {
        Self {
            config,
            partitions: DashMap::new(),
            partition_to_queue: DashMap::new(),
            all_partition_ids: RwLock::new(Vec::new()),
            sequencer_notify,
        }
    }

    /// Register a queue with `num_partitions` partitions `[0, num_partitions)`.
    ///
    /// Idempotent when the partition count matches. Returns
    /// [`OutboxError::PartitionCountMismatch`] if the count differs.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails or if the partition
    /// count does not match an existing registration.
    ///
    /// # Concurrency note
    ///
    /// There is a TOCTOU window between the partition-count check and the
    /// INSERT. Concurrent calls to `register_queue` for the same queue name
    /// with different partition counts may both succeed. This is acceptable
    /// because queue registration is a startup-time operation, not a hot path.
    pub async fn register_queue(
        &self,
        db: &Db,
        queue: &str,
        num_partitions: u16,
    ) -> Result<(), OutboxError> {
        let conn = db.sea_internal();
        let backend = conn.get_database_backend();
        let dialect = Dialect::from(backend);

        // Check existing partition count first — reject if mismatch
        let existing = PartitionRow::find_by_statement(Statement::from_sql_and_values(
            backend,
            dialect.register_queue_select(),
            [queue.into()],
        ))
        .all(&conn)
        .await?;

        if !existing.is_empty() && existing.len() != usize::from(num_partitions) {
            return Err(OutboxError::PartitionCountMismatch {
                queue: queue.to_owned(),
                expected: num_partitions,
                found: existing.len(),
            });
        }

        if !existing.is_empty() {
            // Already registered with correct count — refresh cache
            // and ensure processor rows exist (idempotent via ON CONFLICT DO NOTHING)
            let ids: Vec<i64> = existing.into_iter().map(|r| r.id).collect();
            for &id in &ids {
                conn.execute(Statement::from_sql_and_values(
                    backend,
                    dialect.insert_processor_row(),
                    [id.into()],
                ))
                .await?;
                self.partition_to_queue.insert(id, queue.to_owned());
            }
            self.partitions.insert(queue.to_owned(), ids);
            self.rebuild_partition_id_cache().await;
            return Ok(());
        }

        // First registration — insert partition rows
        for p in 0..num_partitions {
            conn.execute(Statement::from_sql_and_values(
                backend,
                dialect.register_queue_insert(),
                #[allow(clippy::cast_possible_wrap)]
                [queue.into(), (p as i16).into()],
            ))
            .await?;
        }

        // Read back inserted rows to populate cache
        let rows = PartitionRow::find_by_statement(Statement::from_sql_and_values(
            backend,
            dialect.register_queue_select(),
            [queue.into()],
        ))
        .all(&conn)
        .await?;

        let ids: Vec<i64> = rows.into_iter().map(|r| r.id).collect();

        // Insert processor rows for each partition
        for &id in &ids {
            conn.execute(Statement::from_sql_and_values(
                backend,
                dialect.insert_processor_row(),
                [id.into()],
            ))
            .await?;
            self.partition_to_queue.insert(id, queue.to_owned());
        }

        self.partitions.insert(queue.to_owned(), ids);
        self.rebuild_partition_id_cache().await;

        Ok(())
    }

    /// Resolve the `partition_id` (PK) for a `(queue, partition)` pair from cache.
    fn resolve_partition(&self, queue: &str, partition: u32) -> Result<i64, OutboxError> {
        let entry = self
            .partitions
            .get(queue)
            .ok_or_else(|| OutboxError::QueueNotRegistered(queue.to_owned()))?;
        let ids = entry.value();
        ids.get(partition as usize)
            .copied()
            .ok_or_else(|| OutboxError::PartitionOutOfRange {
                queue: queue.to_owned(),
                partition,
                #[allow(clippy::cast_possible_truncation)]
                max: ids.len() as u32,
            })
    }

    /// Maximum payload size in bytes (64 KiB). Hardcoded limit.
    const MAX_PAYLOAD_SIZE: usize = 64 * 1024;

    /// Validate payload size.
    fn validate_payload(payload: &[u8]) -> Result<(), OutboxError> {
        if payload.len() > Self::MAX_PAYLOAD_SIZE {
            return Err(OutboxError::PayloadTooLarge {
                size: payload.len(),
                max: Self::MAX_PAYLOAD_SIZE,
            });
        }
        Ok(())
    }

    /// Enqueue a single item. Accepts `&impl DBRunner` — use within a transaction
    /// for atomicity with business data, or with a standalone connection.
    ///
    /// # Errors
    ///
    /// Returns an error on validation failure or database error.
    pub async fn enqueue(
        &self,
        db: &(impl crate::secure::DBRunner + Sync),
        queue: &str,
        partition: u32,
        payload: Vec<u8>,
        payload_type: &str,
    ) -> Result<OutboxItemId, OutboxError> {
        Self::validate_payload(&payload)?;
        let partition_id = self.resolve_partition(queue, partition)?;

        let runner = db.as_seaorm();
        let incoming_id =
            Self::insert_body_and_incoming(&runner, partition_id, &payload, payload_type).await?;

        Ok(OutboxItemId(incoming_id))
    }

    /// Enqueue a batch of items for a single queue.
    /// All validation happens before any DB writes — a single invalid item
    /// rejects the entire batch.
    ///
    /// # Errors
    ///
    /// Returns an error on validation failure or database error.
    pub async fn enqueue_batch(
        &self,
        db: &(impl crate::secure::DBRunner + Sync),
        queue: &str,
        items: &[(u32, Vec<u8>, &str)],
    ) -> Result<Vec<OutboxItemId>, OutboxError> {
        // Validate ALL items first
        let mut resolved = Vec::with_capacity(items.len());
        for (partition, payload, _payload_type) in items {
            Self::validate_payload(payload)?;
            let partition_id = self.resolve_partition(queue, *partition)?;
            resolved.push(partition_id);
        }

        let runner = db.as_seaorm();
        let ids = Self::insert_batch(&runner, &resolved, items).await?;

        Ok(ids)
    }

    /// Max rows per multi-row INSERT statement to avoid parameter limits.
    const BATCH_CHUNK_SIZE: usize = 100;

    /// Insert a batch of body + incoming rows using multi-row INSERTs.
    async fn insert_batch(
        runner: &SeaOrmRunner<'_>,
        partition_ids: &[i64],
        items: &[(u32, Vec<u8>, &str)],
    ) -> Result<Vec<OutboxItemId>, OutboxError> {
        let (conn, backend): (&dyn ConnectionTrait, DbBackend) = match runner {
            SeaOrmRunner::Conn(c) => (*c, c.get_database_backend()),
            SeaOrmRunner::Tx(t) => (*t, t.get_database_backend()),
        };
        let dialect = Dialect::from(backend);

        if items.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_body_ids: Vec<i64> = Vec::with_capacity(items.len());

        // Insert body rows in chunks
        for chunk in items.chunks(Self::BATCH_CHUNK_SIZE) {
            let sql = dialect.build_insert_body_batch(chunk.len());
            let mut values: Vec<sea_orm::Value> = Vec::with_capacity(chunk.len() * 2);
            for (_partition, payload, payload_type) in chunk {
                values.push(payload.clone().into());
                values.push((*payload_type).into());
            }

            if dialect.supports_returning() {
                let rows = conn
                    .query_all(Statement::from_sql_and_values(backend, &sql, values))
                    .await?;
                for row in rows {
                    let id: i64 = row.try_get_by_index(0).map_err(|e| {
                        OutboxError::Database(sea_orm::DbErr::Custom(format!(
                            "body id column: {e}"
                        )))
                    })?;
                    all_body_ids.push(id);
                }
            } else {
                // MySQL: INSERT then LAST_INSERT_ID() for first ID, consecutive for rest
                conn.execute(Statement::from_sql_and_values(backend, &sql, values))
                    .await?;
                let row = conn
                    .query_one(Statement::from_string(backend, Dialect::last_insert_id()))
                    .await?
                    .ok_or_else(|| {
                        OutboxError::Database(sea_orm::DbErr::Custom(
                            "LAST_INSERT_ID() returned no row for body batch".to_owned(),
                        ))
                    })?;
                let first_id: i64 = row.try_get_by_index(0).map_err(|e| {
                    OutboxError::Database(sea_orm::DbErr::Custom(format!(
                        "body first_id column: {e}"
                    )))
                })?;
                for i in 0..chunk.len() {
                    #[allow(clippy::cast_possible_wrap)]
                    all_body_ids.push(first_id + i as i64);
                }
            }
        }

        let mut all_incoming_ids: Vec<OutboxItemId> = Vec::with_capacity(items.len());

        // Insert incoming rows in chunks
        for chunk_start in (0..items.len()).step_by(Self::BATCH_CHUNK_SIZE) {
            let chunk_end = (chunk_start + Self::BATCH_CHUNK_SIZE).min(items.len());
            let chunk_len = chunk_end - chunk_start;

            let sql = dialect.build_insert_incoming_batch(chunk_len);
            let mut values: Vec<sea_orm::Value> = Vec::with_capacity(chunk_len * 2);
            for i in 0..chunk_len {
                values.push(partition_ids[chunk_start + i].into());
                values.push(all_body_ids[chunk_start + i].into());
            }

            if dialect.supports_returning() {
                let rows = conn
                    .query_all(Statement::from_sql_and_values(backend, &sql, values))
                    .await?;
                for row in rows {
                    let id: i64 = row.try_get_by_index(0).map_err(|e| {
                        OutboxError::Database(sea_orm::DbErr::Custom(format!(
                            "incoming id column: {e}"
                        )))
                    })?;
                    all_incoming_ids.push(OutboxItemId(id));
                }
            } else {
                // MySQL: INSERT then LAST_INSERT_ID() for first ID, consecutive for rest
                conn.execute(Statement::from_sql_and_values(backend, &sql, values))
                    .await?;
                let row = conn
                    .query_one(Statement::from_string(backend, Dialect::last_insert_id()))
                    .await?
                    .ok_or_else(|| {
                        OutboxError::Database(sea_orm::DbErr::Custom(
                            "LAST_INSERT_ID() returned no row for incoming batch".to_owned(),
                        ))
                    })?;
                let first_id: i64 = row.try_get_by_index(0).map_err(|e| {
                    OutboxError::Database(sea_orm::DbErr::Custom(format!(
                        "incoming first_id column: {e}"
                    )))
                })?;
                for i in 0..chunk_len {
                    #[allow(clippy::cast_possible_wrap)]
                    all_incoming_ids.push(OutboxItemId(first_id + i as i64));
                }
            }
        }

        Ok(all_incoming_ids)
    }

    /// Insert body + incoming rows, returning the `incoming_id`.
    async fn insert_body_and_incoming(
        runner: &SeaOrmRunner<'_>,
        partition_id: i64,
        payload: &[u8],
        payload_type: &str,
    ) -> Result<i64, OutboxError> {
        let (conn, backend): (&dyn ConnectionTrait, DbBackend) = match runner {
            SeaOrmRunner::Conn(c) => (*c, c.get_database_backend()),
            SeaOrmRunner::Tx(t) => (*t, t.get_database_backend()),
        };
        let dialect = Dialect::from(backend);

        // 1. INSERT body
        let body_id: i64 = if dialect.supports_returning() {
            let row = conn
                .query_one(Statement::from_sql_and_values(
                    backend,
                    dialect.insert_body(),
                    [payload.to_vec().into(), payload_type.into()],
                ))
                .await?
                .ok_or_else(|| {
                    OutboxError::Database(sea_orm::DbErr::Custom(
                        "INSERT RETURNING returned no row for body".to_owned(),
                    ))
                })?;
            row.try_get_by_index(0).map_err(|e| {
                OutboxError::Database(sea_orm::DbErr::Custom(format!("body id column: {e}")))
            })?
        } else {
            conn.execute(Statement::from_sql_and_values(
                backend,
                dialect.insert_body(),
                [payload.to_vec().into(), payload_type.into()],
            ))
            .await?;
            let row = conn
                .query_one(Statement::from_string(backend, Dialect::last_insert_id()))
                .await?
                .ok_or_else(|| {
                    OutboxError::Database(sea_orm::DbErr::Custom(
                        "LAST_INSERT_ID() returned no row for body".to_owned(),
                    ))
                })?;
            row.try_get_by_index(0).map_err(|e| {
                OutboxError::Database(sea_orm::DbErr::Custom(format!("body id column: {e}")))
            })?
        };

        // 2. INSERT incoming
        let incoming_id: i64 = if dialect.supports_returning() {
            let row = conn
                .query_one(Statement::from_sql_and_values(
                    backend,
                    dialect.insert_incoming(),
                    [partition_id.into(), body_id.into()],
                ))
                .await?
                .ok_or_else(|| {
                    OutboxError::Database(sea_orm::DbErr::Custom(
                        "INSERT RETURNING returned no row for incoming".to_owned(),
                    ))
                })?;
            row.try_get_by_index(0).map_err(|e| {
                OutboxError::Database(sea_orm::DbErr::Custom(format!("incoming id column: {e}")))
            })?
        } else {
            conn.execute(Statement::from_sql_and_values(
                backend,
                dialect.insert_incoming(),
                [partition_id.into(), body_id.into()],
            ))
            .await?;
            let row = conn
                .query_one(Statement::from_string(backend, Dialect::last_insert_id()))
                .await?
                .ok_or_else(|| {
                    OutboxError::Database(sea_orm::DbErr::Custom(
                        "LAST_INSERT_ID() returned no row for incoming".to_owned(),
                    ))
                })?;
            row.try_get_by_index(0).map_err(|e| {
                OutboxError::Database(sea_orm::DbErr::Custom(format!("incoming id column: {e}")))
            })?
        };

        Ok(incoming_id)
    }

    /// Notify the sequencer that new items are available.
    /// Multiple flushes coalesce into a single wakeup.
    pub fn flush(&self) {
        self.sequencer_notify.notify_one();
    }

    /// Execute a closure inside a database transaction, then auto-flush
    /// the sequencer notification channel on success.
    pub async fn transaction<F, T>(&self, db: Db, f: F) -> (Db, anyhow::Result<T>)
    where
        F: for<'a> FnOnce(
                &'a crate::DbTx<'a>,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send + 'a>,
            > + Send,
        T: Send + 'static,
    {
        let (db, result) = db.transaction(f).await;
        if result.is_ok() {
            self.flush();
        }
        (db, result)
    }

    /// Returns all registered partition IDs in deterministic order (sorted by PK).
    /// Reads from a pre-computed cache that is rebuilt on each `register_queue` call.
    pub(crate) fn all_partition_ids(&self) -> Vec<i64> {
        // try_read is non-blocking and always succeeds when no writer is active.
        // Writers only hold the lock briefly during register_queue (startup).
        self.all_partition_ids
            .try_read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Rebuild the flattened partition ID cache from the `DashMap`.
    async fn rebuild_partition_id_cache(&self) {
        let mut ids: Vec<i64> = self
            .partitions
            .iter()
            .flat_map(|entry| entry.value().clone())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        *self.all_partition_ids.write().await = ids;
    }

    /// Access the outbox config.
    #[must_use]
    pub fn config(&self) -> &OutboxConfig {
        &self.config
    }

    /// Returns the partition IDs for a specific queue, in order.
    #[must_use]
    pub(crate) fn partition_ids_for_queue(&self, queue: &str) -> Vec<i64> {
        self.partitions
            .get(queue)
            .map(|v| v.value().clone())
            .unwrap_or_default()
    }

    /// Look up the queue name for a partition ID.
    #[must_use]
    pub fn partition_to_queue(&self, partition_id: i64) -> Option<String> {
        self.partition_to_queue
            .get(&partition_id)
            .map(|v| v.clone())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::outbox::types::*;

    fn make_outbox(config: OutboxConfig) -> Arc<Outbox> {
        let notify = Arc::new(Notify::new());
        Arc::new(Outbox::new(config, notify))
    }

    fn make_default_outbox() -> Arc<Outbox> {
        make_outbox(OutboxConfig::default())
    }

    // -- resolve_partition tests --

    #[test]
    fn resolve_partition_cache_hit() {
        let outbox = make_default_outbox();
        outbox
            .partitions
            .insert("orders".to_owned(), vec![10, 20, 30]);

        assert_eq!(outbox.resolve_partition("orders", 0).unwrap(), 10);
        assert_eq!(outbox.resolve_partition("orders", 1).unwrap(), 20);
        assert_eq!(outbox.resolve_partition("orders", 2).unwrap(), 30);
    }

    #[test]
    fn resolve_partition_unregistered_queue() {
        let outbox = make_default_outbox();

        let err = outbox.resolve_partition("nonexistent", 0).unwrap_err();
        assert!(matches!(err, OutboxError::QueueNotRegistered(q) if q == "nonexistent"));
    }

    #[test]
    fn resolve_partition_out_of_range() {
        let outbox = make_default_outbox();
        outbox
            .partitions
            .insert("orders".to_owned(), vec![10, 20, 30]);

        let err = outbox.resolve_partition("orders", 3).unwrap_err();
        assert!(matches!(
            err,
            OutboxError::PartitionOutOfRange { queue, partition: 3, max: 3 } if queue == "orders"
        ));
    }

    // -- validate_payload tests --

    #[test]
    fn validate_payload_oversized() {
        let oversized = vec![0u8; Outbox::MAX_PAYLOAD_SIZE + 1];
        let err = Outbox::validate_payload(&oversized).unwrap_err();
        assert!(matches!(err, OutboxError::PayloadTooLarge { .. }));
    }

    #[test]
    fn validate_payload_at_exact_limit() {
        let exact = vec![0u8; Outbox::MAX_PAYLOAD_SIZE];
        assert!(Outbox::validate_payload(&exact).is_ok());
    }

    #[test]
    fn validate_payload_empty() {
        assert!(Outbox::validate_payload(&[]).is_ok());
    }

    // -- enqueue_batch validation tests (no DB needed) --

    #[tokio::test]
    async fn enqueue_batch_rejects_out_of_range_partition() {
        let outbox = make_default_outbox();
        outbox.partitions.insert("q".to_owned(), vec![10, 20]);

        let err = outbox.resolve_partition("q", 5).unwrap_err();
        assert!(matches!(err, OutboxError::PartitionOutOfRange { .. }));
    }

    #[tokio::test]
    async fn enqueue_batch_rejects_oversized_payload() {
        let oversized = vec![0u8; Outbox::MAX_PAYLOAD_SIZE + 1];
        let err = Outbox::validate_payload(&oversized).unwrap_err();
        assert!(matches!(err, OutboxError::PayloadTooLarge { .. }));
    }

    // -- flush tests --

    #[tokio::test]
    async fn flush_triggers_notify() {
        let notify = Arc::new(Notify::new());
        let outbox = Arc::new(Outbox::new(OutboxConfig::default(), Arc::clone(&notify)));

        outbox.flush();
        // Notify was signaled — notified() resolves immediately
        tokio::time::timeout(std::time::Duration::from_millis(50), notify.notified())
            .await
            .expect("notify should fire");
    }

    #[test]
    fn flush_does_not_block() {
        let outbox = make_default_outbox();
        // Multiple flushes should not block or panic
        outbox.flush();
        outbox.flush();
        outbox.flush();
    }

    // -- config defaults test --

    #[test]
    fn config_defaults_match_constants() {
        let config = OutboxConfig::default();
        assert_eq!(config.sequencer.batch_size, DEFAULT_SEQUENCER_BATCH_SIZE);
        assert_eq!(config.sequencer.poll_interval, DEFAULT_POLL_INTERVAL);
    }
}
