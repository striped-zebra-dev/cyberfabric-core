use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};
use sea_orm_migration::prelude::*;

// --- Migration: modkit_outbox_body ---

struct CreateOutboxBody;

impl MigrationName for CreateOutboxBody {
    fn name(&self) -> &'static str {
        "m001_create_modkit_outbox_body"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for CreateOutboxBody {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        let sql = match backend {
            DatabaseBackend::Postgres => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_body (
                    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    payload       BYTEA  NOT NULL,
                    payload_type  TEXT   NOT NULL,
                    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
                )"
            }
            DatabaseBackend::Sqlite => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_body (
                    id            INTEGER PRIMARY KEY AUTOINCREMENT,
                    payload       BLOB   NOT NULL,
                    payload_type  TEXT   NOT NULL,
                    created_at    TEXT   NOT NULL DEFAULT (datetime('now'))
                )"
            }
            DatabaseBackend::MySql => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_body (
                    id            BIGINT AUTO_INCREMENT PRIMARY KEY,
                    payload       LONGBLOB NOT NULL,
                    payload_type  TEXT     NOT NULL,
                    created_at    TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)
                )"
            }
        };
        conn.execute(Statement::from_string(backend, sql)).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        conn.execute(Statement::from_string(
            backend,
            "DROP TABLE IF EXISTS modkit_outbox_body",
        ))
        .await?;
        Ok(())
    }
}

// --- Migration: modkit_outbox_partitions ---

struct CreateOutboxPartitions;

impl MigrationName for CreateOutboxPartitions {
    fn name(&self) -> &'static str {
        "m002_create_modkit_outbox_partitions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for CreateOutboxPartitions {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        let sql = match backend {
            DatabaseBackend::Postgres => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_partitions (
                    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    queue               TEXT     NOT NULL,
                    partition           SMALLINT NOT NULL,
                    sequence            BIGINT   NOT NULL DEFAULT 0,
                    processed_sequence  BIGINT   NOT NULL DEFAULT 0,
                    attempts            SMALLINT NOT NULL DEFAULT 0,
                    UNIQUE (queue, partition)
                )"
            }
            DatabaseBackend::Sqlite => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_partitions (
                    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                    queue               TEXT     NOT NULL,
                    partition           INTEGER  NOT NULL,
                    sequence            INTEGER  NOT NULL DEFAULT 0,
                    processed_sequence  INTEGER  NOT NULL DEFAULT 0,
                    attempts            INTEGER  NOT NULL DEFAULT 0,
                    UNIQUE (queue, partition)
                )"
            }
            DatabaseBackend::MySql => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_partitions (
                    id                  BIGINT AUTO_INCREMENT PRIMARY KEY,
                    queue               VARCHAR(255) NOT NULL,
                    `partition`         SMALLINT NOT NULL,
                    sequence            BIGINT   NOT NULL DEFAULT 0,
                    processed_sequence  BIGINT   NOT NULL DEFAULT 0,
                    attempts            SMALLINT NOT NULL DEFAULT 0,
                    UNIQUE KEY (queue, `partition`)
                )"
            }
        };
        conn.execute(Statement::from_string(backend, sql)).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        conn.execute(Statement::from_string(
            backend,
            "DROP TABLE IF EXISTS modkit_outbox_partitions",
        ))
        .await?;
        Ok(())
    }
}

// --- Migration: modkit_outbox_incoming ---

struct CreateOutboxIncoming;

impl MigrationName for CreateOutboxIncoming {
    fn name(&self) -> &'static str {
        "m003_create_modkit_outbox_incoming"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for CreateOutboxIncoming {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        let sql = match backend {
            DatabaseBackend::Postgres => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_incoming (
                    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    partition_id    BIGINT   NOT NULL REFERENCES modkit_outbox_partitions(id),
                    body_id         BIGINT   NOT NULL REFERENCES modkit_outbox_body(id),
                    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
                )"
            }
            DatabaseBackend::Sqlite => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_incoming (
                    id              INTEGER PRIMARY KEY AUTOINCREMENT,
                    partition_id    INTEGER NOT NULL REFERENCES modkit_outbox_partitions(id),
                    body_id         INTEGER NOT NULL REFERENCES modkit_outbox_body(id),
                    created_at      TEXT    NOT NULL DEFAULT (datetime('now'))
                )"
            }
            DatabaseBackend::MySql => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_incoming (
                    id              BIGINT AUTO_INCREMENT PRIMARY KEY,
                    partition_id    BIGINT NOT NULL,
                    body_id         BIGINT NOT NULL,
                    created_at      TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
                    FOREIGN KEY (partition_id) REFERENCES modkit_outbox_partitions(id),
                    FOREIGN KEY (body_id) REFERENCES modkit_outbox_body(id)
                )"
            }
        };
        conn.execute(Statement::from_string(backend, sql)).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        conn.execute(Statement::from_string(
            backend,
            "DROP TABLE IF EXISTS modkit_outbox_incoming",
        ))
        .await?;
        Ok(())
    }
}

// --- Migration: modkit_outbox_outgoing ---

struct CreateOutboxOutgoing;

impl MigrationName for CreateOutboxOutgoing {
    fn name(&self) -> &'static str {
        "m004_create_modkit_outbox_outgoing"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for CreateOutboxOutgoing {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        let sql = match backend {
            DatabaseBackend::Postgres => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_outgoing (
                    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    partition_id    BIGINT NOT NULL REFERENCES modkit_outbox_partitions(id),
                    body_id         BIGINT NOT NULL REFERENCES modkit_outbox_body(id),
                    seq             BIGINT NOT NULL,
                    created_at      TIMESTAMPTZ NOT NULL,
                    sequenced_at    TIMESTAMPTZ NOT NULL DEFAULT now()
                )"
            }
            DatabaseBackend::Sqlite => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_outgoing (
                    id              INTEGER PRIMARY KEY AUTOINCREMENT,
                    partition_id    INTEGER NOT NULL REFERENCES modkit_outbox_partitions(id),
                    body_id         INTEGER NOT NULL REFERENCES modkit_outbox_body(id),
                    seq             INTEGER NOT NULL,
                    created_at      TEXT    NOT NULL,
                    sequenced_at    TEXT    NOT NULL DEFAULT (datetime('now'))
                )"
            }
            DatabaseBackend::MySql => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_outgoing (
                    id              BIGINT AUTO_INCREMENT PRIMARY KEY,
                    partition_id    BIGINT NOT NULL,
                    body_id         BIGINT NOT NULL,
                    seq             BIGINT NOT NULL,
                    created_at      TIMESTAMP(6) NOT NULL,
                    sequenced_at    TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
                    FOREIGN KEY (partition_id) REFERENCES modkit_outbox_partitions(id),
                    FOREIGN KEY (body_id) REFERENCES modkit_outbox_body(id)
                )"
            }
        };
        conn.execute(Statement::from_string(backend, sql)).await?;

        // Unique index on (partition_id, seq)
        let idx_sql = match backend {
            DatabaseBackend::Postgres | DatabaseBackend::Sqlite => {
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_modkit_outbox_outgoing_partition_seq \
                 ON modkit_outbox_outgoing (partition_id, seq)"
            }
            DatabaseBackend::MySql => {
                "CREATE UNIQUE INDEX idx_modkit_outbox_outgoing_partition_seq \
                 ON modkit_outbox_outgoing (partition_id, seq)"
            }
        };
        conn.execute(Statement::from_string(backend, idx_sql))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        conn.execute(Statement::from_string(
            backend,
            "DROP TABLE IF EXISTS modkit_outbox_outgoing",
        ))
        .await?;
        Ok(())
    }
}

// --- Migration: modkit_outbox_dead_letters ---

struct CreateOutboxDeadLetters;

impl MigrationName for CreateOutboxDeadLetters {
    fn name(&self) -> &'static str {
        "m005_create_modkit_outbox_dead_letters"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for CreateOutboxDeadLetters {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        let sql = match backend {
            DatabaseBackend::Postgres => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_dead_letters (
                    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    partition_id    BIGINT NOT NULL REFERENCES modkit_outbox_partitions(id),
                    seq             BIGINT NOT NULL,
                    payload         BYTEA  NOT NULL,
                    payload_type    TEXT   NOT NULL,
                    created_at      TIMESTAMPTZ NOT NULL,
                    failed_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
                    last_error      TEXT,
                    attempts        SMALLINT NOT NULL,
                    replayed_at     TIMESTAMPTZ
                )"
            }
            DatabaseBackend::Sqlite => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_dead_letters (
                    id              INTEGER PRIMARY KEY AUTOINCREMENT,
                    partition_id    INTEGER NOT NULL REFERENCES modkit_outbox_partitions(id),
                    seq             INTEGER NOT NULL,
                    payload         BLOB   NOT NULL,
                    payload_type    TEXT   NOT NULL,
                    created_at      TEXT    NOT NULL,
                    failed_at       TEXT    NOT NULL DEFAULT (datetime('now')),
                    last_error      TEXT,
                    attempts        INTEGER NOT NULL,
                    replayed_at     TEXT
                )"
            }
            DatabaseBackend::MySql => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_dead_letters (
                    id              BIGINT AUTO_INCREMENT PRIMARY KEY,
                    partition_id    BIGINT NOT NULL,
                    seq             BIGINT NOT NULL,
                    payload         LONGBLOB NOT NULL,
                    payload_type    TEXT     NOT NULL,
                    created_at      TIMESTAMP(6) NOT NULL,
                    failed_at       TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
                    last_error      TEXT,
                    attempts        SMALLINT NOT NULL,
                    replayed_at     TIMESTAMP(6) NULL,
                    FOREIGN KEY (partition_id) REFERENCES modkit_outbox_partitions(id)
                )"
            }
        };
        conn.execute(Statement::from_string(backend, sql)).await?;

        // Partial index for pending dead letters
        let idx_sql = match backend {
            DatabaseBackend::Postgres | DatabaseBackend::Sqlite => {
                "CREATE INDEX IF NOT EXISTS idx_modkit_outbox_dead_letters_pending \
                 ON modkit_outbox_dead_letters (failed_at) \
                 WHERE replayed_at IS NULL"
            }
            DatabaseBackend::MySql => {
                // MySQL doesn't support partial indexes; use a full index
                "CREATE INDEX IF NOT EXISTS idx_modkit_outbox_dead_letters_pending \
                 ON modkit_outbox_dead_letters (failed_at, replayed_at)"
            }
        };
        conn.execute(Statement::from_string(backend, idx_sql))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();
        conn.execute(Statement::from_string(
            backend,
            "DROP TABLE IF EXISTS modkit_outbox_dead_letters",
        ))
        .await?;
        Ok(())
    }
}

// --- Migration: modkit_outbox_processor ---

struct CreateOutboxProcessor;

impl MigrationName for CreateOutboxProcessor {
    fn name(&self) -> &'static str {
        "m006_create_modkit_outbox_processor"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for CreateOutboxProcessor {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();

        // 1. Create the processor table
        let create_sql = match backend {
            DatabaseBackend::Postgres => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_processor (
                    partition_id    BIGINT PRIMARY KEY REFERENCES modkit_outbox_partitions(id),
                    processed_seq   BIGINT   NOT NULL DEFAULT 0,
                    attempts        SMALLINT NOT NULL DEFAULT 0,
                    last_error      TEXT,
                    locked_by       TEXT,
                    locked_until    TIMESTAMPTZ
                )"
            }
            DatabaseBackend::Sqlite => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_processor (
                    partition_id    INTEGER PRIMARY KEY REFERENCES modkit_outbox_partitions(id),
                    processed_seq   INTEGER NOT NULL DEFAULT 0,
                    attempts        INTEGER NOT NULL DEFAULT 0,
                    last_error      TEXT,
                    locked_by       TEXT,
                    locked_until    TEXT
                )"
            }
            DatabaseBackend::MySql => {
                "CREATE TABLE IF NOT EXISTS modkit_outbox_processor (
                    partition_id    BIGINT PRIMARY KEY,
                    processed_seq   BIGINT   NOT NULL DEFAULT 0,
                    attempts        SMALLINT NOT NULL DEFAULT 0,
                    last_error      TEXT,
                    locked_by       TEXT,
                    locked_until    TIMESTAMP(6) NULL,
                    FOREIGN KEY (partition_id) REFERENCES modkit_outbox_partitions(id)
                )"
            }
        };
        conn.execute(Statement::from_string(backend, create_sql))
            .await?;

        // 2. Copy existing data from partitions (if columns exist)
        let copy_sql = match backend {
            DatabaseBackend::Postgres => {
                "INSERT INTO modkit_outbox_processor (partition_id, processed_seq, attempts)
                 SELECT id, processed_sequence, attempts FROM modkit_outbox_partitions
                 ON CONFLICT (partition_id) DO NOTHING"
            }
            DatabaseBackend::Sqlite => {
                "INSERT OR IGNORE INTO modkit_outbox_processor (partition_id, processed_seq, attempts)
                 SELECT id, processed_sequence, attempts FROM modkit_outbox_partitions"
            }
            DatabaseBackend::MySql => {
                "INSERT IGNORE INTO modkit_outbox_processor (partition_id, processed_seq, attempts)
                 SELECT id, processed_sequence, attempts FROM modkit_outbox_partitions"
            }
        };
        // Best-effort copy — if the columns don't exist this is a fresh install
        let _r = conn
            .execute(Statement::from_string(backend, copy_sql))
            .await;

        // 3. Drop old columns from partitions table
        match backend {
            DatabaseBackend::Postgres => {
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions DROP COLUMN IF EXISTS processed_sequence",
                    ))
                    .await;
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions DROP COLUMN IF EXISTS attempts",
                    ))
                    .await;
            }
            DatabaseBackend::Sqlite => {
                // SQLite ≥3.35 supports ALTER TABLE DROP COLUMN
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions DROP COLUMN processed_sequence",
                    ))
                    .await;
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions DROP COLUMN attempts",
                    ))
                    .await;
            }
            DatabaseBackend::MySql => {
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions DROP COLUMN processed_sequence",
                    ))
                    .await;
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions DROP COLUMN attempts",
                    ))
                    .await;
            }
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();

        // Re-add columns to partitions
        match backend {
            DatabaseBackend::Postgres => {
                conn.execute(Statement::from_string(
                    backend,
                    "ALTER TABLE modkit_outbox_partitions \
                     ADD COLUMN IF NOT EXISTS processed_sequence BIGINT NOT NULL DEFAULT 0",
                ))
                .await?;
                conn.execute(Statement::from_string(
                    backend,
                    "ALTER TABLE modkit_outbox_partitions \
                     ADD COLUMN IF NOT EXISTS attempts SMALLINT NOT NULL DEFAULT 0",
                ))
                .await?;
            }
            DatabaseBackend::Sqlite => {
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions \
                         ADD COLUMN processed_sequence INTEGER NOT NULL DEFAULT 0",
                    ))
                    .await;
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions \
                         ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0",
                    ))
                    .await;
            }
            DatabaseBackend::MySql => {
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions \
                         ADD COLUMN processed_sequence BIGINT NOT NULL DEFAULT 0",
                    ))
                    .await;
                let _r = conn
                    .execute(Statement::from_string(
                        backend,
                        "ALTER TABLE modkit_outbox_partitions \
                         ADD COLUMN attempts SMALLINT NOT NULL DEFAULT 0",
                    ))
                    .await;
            }
        }

        // Copy data back from processor table
        let copy_back = match backend {
            DatabaseBackend::Postgres => {
                "UPDATE modkit_outbox_partitions p \
                 SET processed_sequence = pr.processed_seq, attempts = pr.attempts \
                 FROM modkit_outbox_processor pr WHERE p.id = pr.partition_id"
            }
            DatabaseBackend::Sqlite => {
                "UPDATE modkit_outbox_partitions \
                 SET processed_sequence = (SELECT processed_seq FROM modkit_outbox_processor WHERE partition_id = modkit_outbox_partitions.id), \
                     attempts = (SELECT attempts FROM modkit_outbox_processor WHERE partition_id = modkit_outbox_partitions.id) \
                 WHERE id IN (SELECT partition_id FROM modkit_outbox_processor)"
            }
            DatabaseBackend::MySql => {
                "UPDATE modkit_outbox_partitions p \
                 INNER JOIN modkit_outbox_processor pr ON p.id = pr.partition_id \
                 SET p.processed_sequence = pr.processed_seq, p.attempts = pr.attempts"
            }
        };
        let _r = conn
            .execute(Statement::from_string(backend, copy_back))
            .await;

        conn.execute(Statement::from_string(
            backend,
            "DROP TABLE IF EXISTS modkit_outbox_processor",
        ))
        .await?;

        Ok(())
    }
}

/// Returns all outbox migrations in dependency order.
#[must_use]
pub fn outbox_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
        Box::new(CreateOutboxBody),
        Box::new(CreateOutboxPartitions),
        Box::new(CreateOutboxIncoming),
        Box::new(CreateOutboxOutgoing),
        Box::new(CreateOutboxDeadLetters),
        Box::new(CreateOutboxProcessor),
    ]
}
