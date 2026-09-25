//! Locally auto-saved compose drafts. Written while typing so nothing is lost if the window is
//! closed or the app crashes; deleted once the mail is queued for sending or discarded.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::now;
use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// None on the first save; the store assigns one.
    pub id: Option<i64>,
    pub account_id: i64,
    pub mode: String,
    pub source_message_id: Option<i64>,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body_html: String,
    pub forward_idx: Vec<i64>,
    pub attachment_paths: Vec<String>,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    account_id: i64,
    mode: String,
    source_message_id: Option<i64>,
    to_addr: String,
    cc_addr: String,
    bcc_addr: String,
    subject: String,
    body_html: String,
    forward_idx_json: String,
    attachment_paths_json: String,
    updated_at: i64,
}

impl From<Row> for Draft {
    fn from(r: Row) -> Self {
        Draft {
            id: Some(r.id),
            account_id: r.account_id,
            mode: r.mode,
            source_message_id: r.source_message_id,
            to: r.to_addr,
            cc: r.cc_addr,
            bcc: r.bcc_addr,
            subject: r.subject,
            body_html: r.body_html,
            forward_idx: serde_json::from_str(&r.forward_idx_json).unwrap_or_default(),
            attachment_paths: serde_json::from_str(&r.attachment_paths_json).unwrap_or_default(),
            updated_at: r.updated_at,
        }
    }
}

const COLUMNS: &str = "id, account_id, mode, source_message_id, to_addr, cc_addr, bcc_addr, subject, body_html,
    forward_idx_json, attachment_paths_json, updated_at";

pub async fn list(pool: &SqlitePool) -> Result<Vec<Draft>> {
    let rows: Vec<Row> = sqlx::query_as(&format!("SELECT {COLUMNS} FROM drafts ORDER BY updated_at DESC"))
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(Draft::from).collect())
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Draft> {
    let row: Row = sqlx::query_as(&format!("SELECT {COLUMNS} FROM drafts WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Entwurf {id}")))?;
    Ok(row.into())
}

/// Inserts or updates a draft and returns its id.
pub async fn save(pool: &SqlitePool, d: &Draft) -> Result<i64> {
    let forward = serde_json::to_string(&d.forward_idx).unwrap_or_else(|_| "[]".into());
    let paths = serde_json::to_string(&d.attachment_paths).unwrap_or_else(|_| "[]".into());
    let updated = match d.id {
        Some(id) => sqlx::query(
            "UPDATE drafts SET account_id = ?, mode = ?, source_message_id = ?, to_addr = ?, cc_addr = ?, bcc_addr = ?,
                subject = ?, body_html = ?, forward_idx_json = ?, attachment_paths_json = ?, updated_at = ? WHERE id = ?",
        )
        .bind(d.account_id)
        .bind(&d.mode)
        .bind(d.source_message_id)
        .bind(&d.to)
        .bind(&d.cc)
        .bind(&d.bcc)
        .bind(&d.subject)
        .bind(&d.body_html)
        .bind(&forward)
        .bind(&paths)
        .bind(now())
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected()
            > 0,
        None => false,
    };
    if let (true, Some(id)) = (updated, d.id) {
        return Ok(id);
    }
    // New draft (or it was deleted meanwhile, e.g. sent from another window): insert.
    Ok(sqlx::query(
        "INSERT INTO drafts (account_id, mode, source_message_id, to_addr, cc_addr, bcc_addr, subject, body_html,
            forward_idx_json, attachment_paths_json, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(d.account_id)
    .bind(&d.mode)
    .bind(d.source_message_id)
    .bind(&d.to)
    .bind(&d.cc)
    .bind(&d.bcc)
    .bind(&d.subject)
    .bind(&d.body_html)
    .bind(&forward)
    .bind(&paths)
    .bind(now())
    .execute(pool)
    .await?
    .last_insert_rowid())
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM drafts WHERE id = ?").bind(id).execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{accounts, open_memory};

    #[tokio::test]
    async fn saves_updates_and_deletes_drafts() {
        let pool = open_memory().await;
        let acc = accounts::insert(
            &pool,
            &accounts::AccountConfig {
                kind: "imap".into(),
                display_name: "T".into(),
                sender_name: String::new(),
                email: "me@x.de".into(),
                imap_host: "h".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                smtp_host: "h".into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                username: "me".into(),
            },
        )
        .await
        .unwrap();

        let mut d = Draft {
            id: None,
            account_id: acc,
            mode: "new".into(),
            source_message_id: None,
            to: "a@x.de".into(),
            cc: String::new(),
            bcc: String::new(),
            subject: "Hallo".into(),
            body_html: "<p>Erster Stand</p>".into(),
            forward_idx: vec![],
            attachment_paths: vec!["C:/tmp/a.pdf".into()],
            updated_at: 0,
        };
        let id = save(&pool, &d).await.unwrap();
        d.id = Some(id);
        d.body_html = "<p>Zweiter Stand</p>".into();
        assert_eq!(save(&pool, &d).await.unwrap(), id, "update keeps the id");

        let all = list(&pool).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].body_html, "<p>Zweiter Stand</p>");
        assert_eq!(all[0].attachment_paths, vec!["C:/tmp/a.pdf"]);

        delete(&pool, id).await.unwrap();
        assert!(list(&pool).await.unwrap().is_empty());
        // Saving a draft that vanished meanwhile recreates it instead of failing.
        assert!(save(&pool, &d).await.unwrap() > 0);
    }
}
