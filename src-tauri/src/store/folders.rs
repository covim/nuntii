use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::{AppError, Result};
use crate::provider::imap::decode_modified_utf7;
use crate::provider::RemoteFolder;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    pub remote_name: String,
    pub display_name: String,
    pub delimiter: Option<String>,
    pub role: Option<String>,
    pub selectable: bool,
    pub unread_count: i64,
    pub total_count: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SyncState {
    pub id: i64,
    pub remote_name: String,
    pub role: Option<String>,
    pub selectable: bool,
    pub uidvalidity: Option<i64>,
    pub uidnext: Option<i64>,
    pub highestmodseq: Option<i64>,
    pub last_exists: Option<i64>,
    pub synced_since: Option<i64>,
}

const LIST_SQL: &str = "SELECT f.id, f.account_id, f.remote_name, f.display_name, f.delimiter, f.role, f.selectable,
        COALESCE(SUM(CASE WHEN m.seen = 0 THEN 1 ELSE 0 END), 0) AS unread_count,
        COUNT(m.id) AS total_count
    FROM folders f JOIN accounts a ON a.id = f.account_id LEFT JOIN messages m ON m.folder_id = f.id";

pub async fn list(pool: &SqlitePool, account_id: Option<i64>) -> Result<Vec<Folder>> {
    let sql = format!(
        "{LIST_SQL} WHERE (?1 IS NULL OR f.account_id = ?1) AND (f.role IS NULL OR f.role != 'virtual')
         GROUP BY f.id
         ORDER BY a.sort_order, f.account_id,
           CASE f.role WHEN 'inbox' THEN 0 WHEN 'drafts' THEN 1 WHEN 'sent' THEN 2 WHEN 'archive' THEN 3
                       WHEN 'junk' THEN 4 WHEN 'trash' THEN 5 ELSE 6 END,
           lower(f.display_name)"
    );
    Ok(sqlx::query_as(&sql).bind(account_id).fetch_all(pool).await?)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Folder> {
    sqlx::query_as(&format!("{LIST_SQL} WHERE f.id = ? GROUP BY f.id"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Ordner {id}")))
}

pub async fn by_role(pool: &SqlitePool, account_id: i64, role: &str) -> Result<Option<SyncState>> {
    Ok(sqlx::query_as(
        "SELECT id, remote_name, role, selectable, uidvalidity, uidnext, highestmodseq, last_exists, synced_since FROM folders WHERE account_id = ? AND role = ? ORDER BY id LIMIT 1",
    )
    .bind(account_id)
    .bind(role)
    .fetch_optional(pool)
    .await?)
}

pub async fn sync_states(pool: &SqlitePool, account_id: i64) -> Result<Vec<SyncState>> {
    Ok(sqlx::query_as(
        "SELECT id, remote_name, role, selectable, uidvalidity, uidnext, highestmodseq, last_exists, synced_since FROM folders WHERE account_id = ?
         ORDER BY CASE role WHEN 'inbox' THEN 0 WHEN 'sent' THEN 1 ELSE 2 END, id",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?)
}

pub async fn sync_state(pool: &SqlitePool, folder_id: i64) -> Result<SyncState> {
    sqlx::query_as(
        "SELECT id, remote_name, role, selectable, uidvalidity, uidnext, highestmodseq, last_exists, synced_since FROM folders WHERE id = ?",
    )
    .bind(folder_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Ordner {folder_id}")))
}

fn display_name(f: &RemoteFolder) -> String {
    let decoded = decode_modified_utf7(&f.name);
    if decoded.eq_ignore_ascii_case("INBOX") {
        return "Posteingang".into();
    }
    decoded
}

/// Upserts the server's folder list and removes folders that no longer exist remotely.
pub async fn reconcile(pool: &SqlitePool, account_id: i64, remote: &[RemoteFolder]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for f in remote {
        sqlx::query(
            "INSERT INTO folders (account_id, remote_name, display_name, delimiter, role, selectable)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(account_id, remote_name) DO UPDATE SET
               display_name = excluded.display_name, delimiter = excluded.delimiter,
               role = excluded.role, selectable = excluded.selectable",
        )
        .bind(account_id)
        .bind(&f.name)
        .bind(display_name(f))
        .bind(&f.delimiter)
        .bind(f.role.map(|r| r.as_str()))
        .bind(f.selectable)
        .execute(&mut *tx)
        .await?;
    }
    let existing: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, remote_name FROM folders WHERE account_id = ?")
            .bind(account_id)
            .fetch_all(&mut *tx)
            .await?;
    for (id, name) in existing {
        if !remote.iter().any(|f| f.name == name) {
            sqlx::query("DELETE FROM folders WHERE id = ?").bind(id).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn update_sync_state(
    pool: &SqlitePool,
    folder_id: i64,
    uidvalidity: u32,
    uidnext: u32,
    highestmodseq: Option<u64>,
    exists: u32,
    synced_since: i64,
) -> Result<()> {
    sqlx::query("UPDATE folders SET uidvalidity = ?, uidnext = ?, highestmodseq = ?, last_exists = ?, synced_since = ?, last_sync = ? WHERE id = ?")
        .bind(uidvalidity as i64)
        .bind(uidnext as i64)
        .bind(highestmodseq.map(|m| m as i64))
        .bind(exists as i64)
        .bind(synced_since)
        .bind(super::now())
        .bind(folder_id)
        .execute(pool)
        .await?;
    Ok(())
}
