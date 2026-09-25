//! Local SQLite cache: accounts, folders, messages (incl. raw bodies for offline reading),
//! threads, signatures, settings and the outbox.

pub mod accounts;
pub mod drafts;
pub mod folders;
pub mod messages;

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use crate::error::Result;

pub async fn open(path: &Path) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
pub async fn open_memory() -> SqlitePool {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:").unwrap().foreign_keys(true);
    // A single connection keeps the in-memory database alive and shared.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---- settings ---------------------------------------------------------------------------------

pub async fn setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?)
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- signatures -------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    pub id: i64,
    pub name: String,
    pub body_text: String,
}

pub async fn signatures(pool: &SqlitePool) -> Result<Vec<Signature>> {
    Ok(sqlx::query_as("SELECT id, name, body_text FROM signatures ORDER BY name")
        .fetch_all(pool)
        .await?)
}

pub async fn save_signature(pool: &SqlitePool, id: Option<i64>, name: &str, body: &str) -> Result<i64> {
    Ok(match id {
        Some(id) => {
            sqlx::query("UPDATE signatures SET name = ?, body_text = ? WHERE id = ?")
                .bind(name)
                .bind(body)
                .bind(id)
                .execute(pool)
                .await?;
            id
        }
        None => sqlx::query("INSERT INTO signatures (name, body_text) VALUES (?, ?)")
            .bind(name)
            .bind(body)
            .execute(pool)
            .await?
            .last_insert_rowid(),
    })
}

pub async fn delete_signature(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM signatures WHERE id = ?").bind(id).execute(pool).await?;
    Ok(())
}

// ---- trusted senders (remote images) ----------------------------------------------------------

pub async fn is_trusted_sender(pool: &SqlitePool, address: &str) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM trusted_senders WHERE address = ?")
        .bind(address.to_lowercase())
        .fetch_one(pool)
        .await?
        > 0)
}

pub async fn trust_sender(pool: &SqlitePool, address: &str) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO trusted_senders (address) VALUES (?)")
        .bind(address.to_lowercase())
        .execute(pool)
        .await?;
    Ok(())
}

// ---- outbox -----------------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxItem {
    pub id: i64,
    pub account_id: i64,
    pub raw: Vec<u8>,
    pub env_from: String,
    pub env_to_json: String,
    pub reply_to_message_id: Option<i64>,
}

pub async fn outbox_add(
    pool: &SqlitePool,
    account_id: i64,
    raw: &[u8],
    env_from: &str,
    env_to: &[String],
    reply_to_message_id: Option<i64>,
) -> Result<i64> {
    Ok(sqlx::query(
        "INSERT INTO outbox (account_id, raw, env_from, env_to_json, reply_to_message_id, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(account_id)
    .bind(raw)
    .bind(env_from)
    .bind(serde_json::to_string(env_to).unwrap_or_default())
    .bind(reply_to_message_id)
    .bind(now())
    .execute(pool)
    .await?
    .last_insert_rowid())
}

pub async fn outbox_pending(pool: &SqlitePool, account_id: i64) -> Result<Vec<OutboxItem>> {
    Ok(sqlx::query_as(
        "SELECT id, account_id, raw, env_from, env_to_json, reply_to_message_id FROM outbox WHERE account_id = ? ORDER BY id",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?)
}

pub async fn outbox_get(pool: &SqlitePool, id: i64) -> Result<Option<OutboxItem>> {
    Ok(sqlx::query_as(
        "SELECT id, account_id, raw, env_from, env_to_json, reply_to_message_id FROM outbox WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn outbox_done(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM outbox WHERE id = ?").bind(id).execute(pool).await?;
    Ok(())
}

pub async fn outbox_failed(pool: &SqlitePool, id: i64, error: &str) -> Result<()> {
    sqlx::query("UPDATE outbox SET status = 'failed', error = ? WHERE id = ?")
        .bind(error)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

