use sea_orm::{ConnectionTrait, DbBackend, FromQueryResult, Statement, TransactionTrait};
use tokio_util::sync::CancellationToken;

use super::dialect::Dialect;
use super::handler::{Handler, HandlerResult, OutboxMessage, TransactionalHandler};
use super::types::{OutboxError, QueueConfig};
use crate::Db;

/// Context for processing a single partition's batch.
pub struct ProcessContext<'a> {
    pub db: &'a Db,
    pub backend: DbBackend,
    pub dialect: Dialect,
    pub partition_id: i64,
}

/// Sealed trait for compile-time processing mode dispatch.
///
/// Each implementation manages its own transaction scope. The processor
/// delegates the entire read→handle→ack cycle to the strategy.
pub trait ProcessingStrategy: Send + Sync {
    /// Process one batch for the given partition.
    ///
    /// Returns `Ok(Some(result))` if work was done, `Ok(None)` if the
    /// partition was empty or locked by another processor.
    fn process(
        &self,
        ctx: &ProcessContext<'_>,
        config: &QueueConfig,
        cancel: CancellationToken,
    ) -> impl std::future::Future<Output = Result<Option<ProcessResult>, OutboxError>> + Send;
}

/// Result of processing a batch.
pub struct ProcessResult {
    pub count: u32,
    pub handler_result: HandlerResult,
    pub attempts_before: i16,
}

// ---- SQL row types ----

#[derive(Debug, FromQueryResult)]
struct ProcessorRow {
    processed_seq: i64,
    attempts: i16,
}

#[derive(Debug, FromQueryResult)]
struct OutgoingRow {
    id: i64,
    body_id: i64,
    seq: i64,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, FromQueryResult)]
struct BodyRow {
    payload: Vec<u8>,
    payload_type: String,
}

// ---- Shared helpers ----

async fn read_messages(
    txn: &impl ConnectionTrait,
    backend: DbBackend,
    dialect: &Dialect,
    partition_id: i64,
    proc_row: &ProcessorRow,
    msg_batch_size: u32,
) -> Result<Vec<OutboxMessage>, OutboxError> {
    let start_seq = proc_row.processed_seq + 1;
    let end_seq = start_seq + i64::from(msg_batch_size);

    let outgoing_rows = OutgoingRow::find_by_statement(Statement::from_sql_and_values(
        backend,
        dialect.read_outgoing_batch(),
        [partition_id.into(), start_seq.into(), end_seq.into()],
    ))
    .all(txn)
    .await?;

    if outgoing_rows.is_empty() {
        return Ok(Vec::new());
    }

    let mut msgs = Vec::with_capacity(outgoing_rows.len());
    for row in &outgoing_rows {
        let body = BodyRow::find_by_statement(Statement::from_sql_and_values(
            backend,
            dialect.read_body(),
            [row.body_id.into()],
        ))
        .one(txn)
        .await?;

        let Some(body) = body else {
            return Err(OutboxError::Database(sea_orm::DbErr::Custom(format!(
                "body row {} not found for outgoing {}",
                row.body_id, row.id
            ))));
        };

        msgs.push(OutboxMessage {
            partition_id,
            seq: row.seq,
            payload: body.payload,
            payload_type: body.payload_type,
            created_at: row.created_at,
            attempts: proc_row.attempts,
        });
    }

    Ok(msgs)
}

/// Append-only ack: only UPDATE `processed_seq`, no DELETEs.
/// Reaper handles cleanup of processed outgoing + body rows.
async fn ack(
    txn: &impl ConnectionTrait,
    backend: DbBackend,
    dialect: &Dialect,
    partition_id: i64,
    msgs: &[OutboxMessage],
    result: &HandlerResult,
) -> Result<(), OutboxError> {
    let last_seq = msgs.last().map_or(0, |m| m.seq);

    match result {
        HandlerResult::Success => {
            txn.execute(Statement::from_sql_and_values(
                backend,
                dialect.advance_processed_seq(),
                [last_seq.into(), partition_id.into()],
            ))
            .await?;
        }
        HandlerResult::Retry { reason } => {
            txn.execute(Statement::from_sql_and_values(
                backend,
                dialect.record_retry(),
                [reason.as_str().into(), partition_id.into()],
            ))
            .await?;
        }
        HandlerResult::Reject { reason } => {
            for msg in msgs {
                txn.execute(Statement::from_sql_and_values(
                    backend,
                    dialect.insert_dead_letter(),
                    [
                        partition_id.into(),
                        msg.seq.into(),
                        msg.payload.clone().into(),
                        msg.payload_type.clone().into(),
                        msg.created_at.into(),
                        reason.as_str().into(),
                        msg.attempts.into(),
                    ],
                ))
                .await?;
            }

            txn.execute(Statement::from_sql_and_values(
                backend,
                dialect.advance_processed_seq(),
                [last_seq.into(), partition_id.into()],
            ))
            .await?;
        }
    }

    Ok(())
}

async fn try_lock_and_read_state(
    txn: &impl ConnectionTrait,
    backend: DbBackend,
    dialect: &Dialect,
    partition_id: i64,
) -> Result<Option<ProcessorRow>, OutboxError> {
    if let Some(lock_sql) = dialect.lock_processor() {
        let row = txn
            .query_one(Statement::from_sql_and_values(
                backend,
                lock_sql,
                [partition_id.into()],
            ))
            .await?;
        if row.is_none() {
            return Ok(None);
        }
    }

    let proc_row = ProcessorRow::find_by_statement(Statement::from_sql_and_values(
        backend,
        dialect.read_processor(),
        [partition_id.into()],
    ))
    .one(txn)
    .await?;

    Ok(proc_row)
}

// ---- Transactional strategy ----

/// Processes messages inside the DB transaction holding the partition lock.
/// Handler can perform atomic DB writes alongside the ack.
pub struct TransactionalStrategy {
    handler: Box<dyn TransactionalHandler>,
}

impl TransactionalStrategy {
    pub fn new(handler: Box<dyn TransactionalHandler>) -> Self {
        Self { handler }
    }
}

impl ProcessingStrategy for TransactionalStrategy {
    async fn process(
        &self,
        ctx: &ProcessContext<'_>,
        config: &QueueConfig,
        cancel: CancellationToken,
    ) -> Result<Option<ProcessResult>, OutboxError> {
        let conn = ctx.db.sea_internal();
        let txn = conn.begin().await?;

        let Some(proc_row) =
            try_lock_and_read_state(&txn, ctx.backend, &ctx.dialect, ctx.partition_id).await?
        else {
            txn.commit().await?;
            return Ok(None);
        };

        let msgs = read_messages(
            &txn,
            ctx.backend,
            &ctx.dialect,
            ctx.partition_id,
            &proc_row,
            config.msg_batch_size,
        )
        .await?;
        if msgs.is_empty() {
            txn.commit().await?;
            return Ok(None);
        }

        #[allow(clippy::cast_possible_truncation)]
        let count = msgs.len() as u32;
        let attempts_before = proc_row.attempts;

        let result = self.handler.handle(&txn, &msgs, cancel).await;

        ack(
            &txn,
            ctx.backend,
            &ctx.dialect,
            ctx.partition_id,
            &msgs,
            &result,
        )
        .await?;

        txn.commit().await?;

        Ok(Some(ProcessResult {
            count,
            handler_result: result,
            attempts_before,
        }))
    }
}

// ---- Decoupled strategy ----

/// Processes messages outside any DB transaction.
/// Uses lease-based 3-phase: acquire lease+read → handle → lease-guarded ack.
pub struct DecoupledStrategy {
    handler: Box<dyn Handler>,
}

impl DecoupledStrategy {
    pub fn new(handler: Box<dyn Handler>) -> Self {
        Self { handler }
    }
}

impl ProcessingStrategy for DecoupledStrategy {
    async fn process(
        &self,
        ctx: &ProcessContext<'_>,
        config: &QueueConfig,
        cancel: CancellationToken,
    ) -> Result<Option<ProcessResult>, OutboxError> {
        // Generate a unique lease ID for this processing cycle
        let lease_id = generate_lease_id();
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let lease_secs = config.lease_duration.as_secs() as i64;

        // Phase 1: Acquire lease + read messages
        let (proc_row, msgs) = {
            let sea_conn = ctx.db.sea_internal();
            let txn = sea_conn.begin().await?;

            // Acquire lease via atomic UPDATE ... RETURNING
            let proc_row = if ctx.dialect.supports_returning() {
                ProcessorRow::find_by_statement(Statement::from_sql_and_values(
                    ctx.backend,
                    ctx.dialect.lease_acquire(),
                    [
                        lease_id.as_str().into(),
                        lease_secs.into(),
                        ctx.partition_id.into(),
                    ],
                ))
                .one(&txn)
                .await?
            } else {
                // MySQL: UPDATE then SELECT
                let result = txn
                    .execute(Statement::from_sql_and_values(
                        ctx.backend,
                        ctx.dialect.lease_acquire(),
                        [
                            lease_id.as_str().into(),
                            lease_secs.into(),
                            ctx.partition_id.into(),
                        ],
                    ))
                    .await?;
                if result.rows_affected() == 0 {
                    None
                } else {
                    ProcessorRow::find_by_statement(Statement::from_sql_and_values(
                        ctx.backend,
                        ctx.dialect.read_processor(),
                        [ctx.partition_id.into()],
                    ))
                    .one(&txn)
                    .await?
                }
            };

            let Some(mut proc_row) = proc_row else {
                txn.commit().await?;
                return Ok(None);
            };

            // lease_acquire increments attempts in the DB so a crash leaves
            // a trace. Subtract 1 so the handler sees the pre-increment
            // value (0 = first attempt, 1 = one previous attempt, etc.).
            proc_row.attempts = proc_row.attempts.saturating_sub(1);

            let msgs = read_messages(
                &txn,
                ctx.backend,
                &ctx.dialect,
                ctx.partition_id,
                &proc_row,
                config.msg_batch_size,
            )
            .await?;

            txn.commit().await?;

            if msgs.is_empty() {
                // Release lease — nothing to process
                let conn = ctx.db.sea_internal();
                conn.execute(Statement::from_sql_and_values(
                    ctx.backend,
                    ctx.dialect.lease_release(),
                    [ctx.partition_id.into(), lease_id.as_str().into()],
                ))
                .await?;
                return Ok(None);
            }

            (proc_row, msgs)
        };

        #[allow(clippy::cast_possible_truncation)]
        let count = msgs.len() as u32;
        let attempts_before = proc_row.attempts;

        // Phase 2: call handler outside any transaction
        // Create a child token that fires at 80% of lease duration
        let lease_cancel = cancel.child_token();
        let lease_timer = {
            let token = lease_cancel.clone();
            let deadline = config.lease_duration.mul_f64(0.8);
            tokio::spawn(async move {
                tokio::time::sleep(deadline).await;
                token.cancel();
            })
        };

        let result = self.handler.handle(&msgs, lease_cancel).await;
        lease_timer.abort();

        // Phase 3: lease-guarded ack
        let ack_conn = ctx.db.sea_internal();
        let ack_txn = ack_conn.begin().await?;

        let last_seq = msgs.last().map_or(0, |m| m.seq);

        match &result {
            HandlerResult::Success => {
                let res = ack_txn
                    .execute(Statement::from_sql_and_values(
                        ctx.backend,
                        ctx.dialect.lease_ack_advance(),
                        [
                            last_seq.into(),
                            ctx.partition_id.into(),
                            lease_id.as_str().into(),
                        ],
                    ))
                    .await?;
                if res.rows_affected() == 0 {
                    tracing::error!(
                        partition_id = ctx.partition_id,
                        "lease expired before ack \u{2014} another processor may have taken over"
                    );
                    ack_txn.commit().await?;
                    return Ok(None);
                }
            }
            HandlerResult::Retry { reason } => {
                let res = ack_txn
                    .execute(Statement::from_sql_and_values(
                        ctx.backend,
                        ctx.dialect.lease_record_retry(),
                        [
                            reason.as_str().into(),
                            ctx.partition_id.into(),
                            lease_id.as_str().into(),
                        ],
                    ))
                    .await?;
                if res.rows_affected() == 0 {
                    tracing::error!(
                        partition_id = ctx.partition_id,
                        "lease expired before retry ack"
                    );
                    ack_txn.commit().await?;
                    return Ok(None);
                }
            }
            HandlerResult::Reject { reason } => {
                for msg in &msgs {
                    ack_txn
                        .execute(Statement::from_sql_and_values(
                            ctx.backend,
                            ctx.dialect.insert_dead_letter(),
                            [
                                ctx.partition_id.into(),
                                msg.seq.into(),
                                msg.payload.clone().into(),
                                msg.payload_type.clone().into(),
                                msg.created_at.into(),
                                reason.as_str().into(),
                                msg.attempts.into(),
                            ],
                        ))
                        .await?;
                }

                let res = ack_txn
                    .execute(Statement::from_sql_and_values(
                        ctx.backend,
                        ctx.dialect.lease_ack_advance(),
                        [
                            last_seq.into(),
                            ctx.partition_id.into(),
                            lease_id.as_str().into(),
                        ],
                    ))
                    .await?;
                if res.rows_affected() == 0 {
                    tracing::error!(
                        partition_id = ctx.partition_id,
                        "lease expired before reject ack"
                    );
                    ack_txn.commit().await?;
                    return Ok(None);
                }
            }
        }

        ack_txn.commit().await?;

        Ok(Some(ProcessResult {
            count,
            handler_result: result,
            attempts_before,
        }))
    }
}

/// Generate a unique lease ID using timestamp + random bits.
fn generate_lease_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:x}")
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn lease_id_is_hex_and_nonempty() {
        let id = generate_lease_id();
        assert!(!id.is_empty());
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "lease ID should be hex: {id}"
        );
    }

    #[test]
    fn lease_ids_are_monotonic() {
        let id1 = generate_lease_id();
        // Small sleep to ensure timestamp advances
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = generate_lease_id();
        // Parse as hex integers — id2 should be >= id1
        let n1 = u128::from_str_radix(&id1, 16).expect("parse id1");
        let n2 = u128::from_str_radix(&id2, 16).expect("parse id2");
        assert!(n2 > n1, "lease IDs should be monotonic: {id1} vs {id2}");
    }
}
