use anyhow::Result;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

use crate::types::{CrossChainMessage, MessageState};

/// Initialize the SQLite database and run migrations.
pub async fn init_db(database_url: &str) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS messages (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            nonce           INTEGER NOT NULL UNIQUE,
            trace_id        TEXT NOT NULL,
            sender          TEXT NOT NULL,
            amount          TEXT NOT NULL,
            payload         TEXT NOT NULL,
            deadline        INTEGER NOT NULL,
            description     TEXT,
            state           TEXT NOT NULL DEFAULT 'observed',
            result          TEXT,
            solana_signature TEXT,
            eth_settle_tx   TEXT,
            proof_json      TEXT,
            retry_count     INTEGER NOT NULL DEFAULT 0,
            error_message   TEXT,
            created_at      TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS events (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            nonce       INTEGER NOT NULL,
            trace_id    TEXT NOT NULL,
            actor       TEXT NOT NULL,
            step        TEXT NOT NULL,
            status      TEXT NOT NULL,
            detail      TEXT,
            timestamp   TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_messages_state ON messages(state)",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_events_nonce ON events(nonce)",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

/// Insert a new cross-chain message.
pub async fn insert_message(
    pool: &SqlitePool,
    nonce: u64,
    trace_id: &str,
    sender: &str,
    amount: &str,
    payload: &str,
    deadline: i64,
    description: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO messages (nonce, trace_id, sender, amount, payload, deadline, description, state)
        VALUES (?, ?, ?, ?, ?, ?, ?, 'observed')
        "#,
    )
    .bind(nonce as i64)
    .bind(trace_id)
    .bind(sender)
    .bind(amount)
    .bind(payload)
    .bind(deadline)
    .bind(description)
    .execute(pool)
    .await?;

    Ok(())
}

/// Update message state with optional fields.
pub async fn update_message_state(
    pool: &SqlitePool,
    nonce: u64,
    new_state: MessageState,
    result: Option<&str>,
    solana_sig: Option<&str>,
    eth_settle_tx: Option<&str>,
    error_msg: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE messages SET
            state = ?,
            result = COALESCE(?, result),
            solana_signature = COALESCE(?, solana_signature),
            eth_settle_tx = COALESCE(?, eth_settle_tx),
            error_message = COALESCE(?, error_message),
            updated_at = datetime('now')
        WHERE nonce = ?
        "#,
    )
    .bind(new_state.to_string())
    .bind(result)
    .bind(solana_sig)
    .bind(eth_settle_tx)
    .bind(error_msg)
    .bind(nonce as i64)
    .execute(pool)
    .await?;

    Ok(())
}

/// Store the proof bundle JSON for a message.
pub async fn store_proof(pool: &SqlitePool, nonce: u64, proof_json: &str) -> Result<()> {
    sqlx::query(
        "UPDATE messages SET proof_json = ?, updated_at = datetime('now') WHERE nonce = ?",
    )
    .bind(proof_json)
    .bind(nonce as i64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Increment retry count for a message.
pub async fn increment_retry(pool: &SqlitePool, nonce: u64) -> Result<()> {
    sqlx::query(
        "UPDATE messages SET retry_count = retry_count + 1, updated_at = datetime('now') WHERE nonce = ?",
    )
    .bind(nonce as i64)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get all messages in a given state (for crash-safe resume).
pub async fn get_messages_by_state(
    pool: &SqlitePool,
    state: MessageState,
) -> Result<Vec<CrossChainMessage>> {
    let state_str = state.to_string();
    let rows = sqlx::query_as::<_, CrossChainMessage>(
        r#"
        SELECT
            id, nonce, trace_id, sender, amount, payload, deadline,
            description, state, result, solana_signature, eth_settle_tx, proof_json,
            retry_count, error_message, created_at, updated_at
        FROM messages
        WHERE state = ?
        ORDER BY nonce ASC
        "#,
    )
    .bind(&state_str)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Get a single message by nonce.
pub async fn get_message_by_nonce(
    pool: &SqlitePool,
    nonce: u64,
) -> Result<Option<CrossChainMessage>> {
    let row = sqlx::query_as::<_, CrossChainMessage>(
        r#"
        SELECT
            id, nonce, trace_id, sender, amount, payload, deadline,
            description, state, result, solana_signature, eth_settle_tx, proof_json,
            retry_count, error_message, created_at, updated_at
        FROM messages
        WHERE nonce = ?
        "#,
    )
    .bind(nonce as i64)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Get all messages ordered by nonce descending.
pub async fn get_all_messages(pool: &SqlitePool) -> Result<Vec<CrossChainMessage>> {
    let rows = sqlx::query_as::<_, CrossChainMessage>(
        r#"
        SELECT
            id, nonce, trace_id, sender, amount, payload, deadline,
            description, state, result, solana_signature, eth_settle_tx, proof_json,
            retry_count, error_message, created_at, updated_at
        FROM messages
        ORDER BY nonce DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Get metrics aggregate (single query).
pub async fn get_metrics(pool: &SqlitePool) -> Result<(i64, i64, i64, i64, i64)> {
    let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) AS total,
            SUM(CASE WHEN state = 'settled' THEN 1 ELSE 0 END) AS settled,
            SUM(CASE WHEN state IN ('failed', 'rolled_back') THEN 1 ELSE 0 END) AS failed,
            SUM(CASE WHEN state NOT IN ('settled', 'failed', 'rolled_back') THEN 1 ELSE 0 END) AS pending,
            COALESCE(SUM(retry_count), 0) AS retries
        FROM messages
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Persist a lifecycle event.
pub async fn insert_event(
    pool: &SqlitePool,
    nonce: u64,
    trace_id: &str,
    actor: &str,
    step: &str,
    status: &str,
    detail: Option<&str>,
    timestamp: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO events (nonce, trace_id, actor, step, status, detail, timestamp)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(nonce as i64)
    .bind(trace_id)
    .bind(actor)
    .bind(step)
    .bind(status)
    .bind(detail)
    .bind(timestamp)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get events for a given nonce.
pub async fn get_events_by_nonce(
    pool: &SqlitePool,
    nonce: u64,
) -> Result<Vec<crate::event::LifecycleEvent>> {
    let rows = sqlx::query_as::<_, EventRow>(
        r#"
        SELECT trace_id, nonce, actor, step, status, detail, timestamp
        FROM events
        WHERE nonce = ?
        ORDER BY id ASC
        "#,
    )
    .bind(nonce as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| crate::event::LifecycleEvent {
            trace_id: r.trace_id,
            nonce: r.nonce as u64,
            actor: match r.actor.as_str() {
                "ethereum" => crate::event::Actor::Ethereum,
                "solana" => crate::event::Actor::Solana,
                "dashboard" => crate::event::Actor::Dashboard,
                _ => crate::event::Actor::Relayer,
            },
            step: match r.step.as_str() {
                "locked" => crate::event::Step::Locked,
                "observed" => crate::event::Step::Observed,
                "verified" => crate::event::Step::Verified,
                "executed" => crate::event::Step::Executed,
                "minted" => crate::event::Step::Minted,
                "burned" => crate::event::Step::Burned,
                _ => crate::event::Step::Settled,
            },
            status: match r.status.as_str() {
                "failure" => crate::event::Status::Failure,
                "retry" => crate::event::Status::Retry,
                _ => crate::event::Status::Success,
            },
            timestamp: r.timestamp,
            detail: r.detail,
        })
        .collect())
}

#[derive(Debug, sqlx::FromRow)]
struct EventRow {
    trace_id: String,
    nonce: i64,
    actor: String,
    step: String,
    status: String,
    detail: Option<String>,
    timestamp: String,
}

/// Delete all messages and events (clear demo data).
pub async fn clear_all_data(pool: &SqlitePool) -> Result<()> {
    sqlx::query("DELETE FROM events").execute(pool).await?;
    sqlx::query("DELETE FROM messages").execute(pool).await?;
    Ok(())
}

/// Check if a nonce already exists (for idempotency).
pub async fn nonce_exists(pool: &SqlitePool, nonce: u64) -> Result<bool> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE nonce = ?")
        .bind(nonce as i64)
        .fetch_one(pool)
        .await?;

    Ok(count > 0)
}
