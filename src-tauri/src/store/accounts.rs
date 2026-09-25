use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::now;
use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: i64,
    pub kind: String,
    /// Account name shown in the app (sidebar, "From" picker). Never sent.
    pub display_name: String,
    /// Name in the From header of sent mail; empty = address only.
    pub sender_name: String,
    pub sort_order: i64,
    pub email: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub imap_security: String,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub smtp_security: String,
    pub username: String,
    pub signature_id: Option<i64>,
}

/// Account settings without id, as entered in the setup dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountConfig {
    pub kind: String,
    pub display_name: String,
    #[serde(default)]
    pub sender_name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub imap_security: String,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub smtp_security: String,
    pub username: String,
}

const COLUMNS: &str = "id, kind, display_name, sender_name, sort_order, email, imap_host, imap_port, imap_security, smtp_host, smtp_port, smtp_security, username, signature_id";

pub async fn list(pool: &SqlitePool) -> Result<Vec<Account>> {
    Ok(sqlx::query_as(&format!("SELECT {COLUMNS} FROM accounts ORDER BY sort_order, id"))
        .fetch_all(pool)
        .await?)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Account> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM accounts WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Konto {id}")))
}

pub async fn insert(pool: &SqlitePool, c: &AccountConfig) -> Result<i64> {
    Ok(sqlx::query(
        "INSERT INTO accounts (kind, display_name, sender_name, email, imap_host, imap_port, imap_security, smtp_host, smtp_port, smtp_security, username, created_at, sort_order)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM accounts))",
    )
    .bind(&c.kind)
    .bind(&c.display_name)
    .bind(c.sender_name.trim())
    .bind(&c.email)
    .bind(&c.imap_host)
    .bind(c.imap_port)
    .bind(&c.imap_security)
    .bind(&c.smtp_host)
    .bind(c.smtp_port)
    .bind(&c.smtp_security)
    .bind(&c.username)
    .bind(now())
    .execute(pool)
    .await?
    .last_insert_rowid())
}

pub async fn update_profile(
    pool: &SqlitePool,
    id: i64,
    display_name: &str,
    sender_name: &str,
    signature_id: Option<i64>,
) -> Result<()> {
    sqlx::query("UPDATE accounts SET display_name = ?, sender_name = ?, signature_id = ? WHERE id = ?")
        .bind(display_name)
        .bind(sender_name)
        .bind(signature_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Persists the user-defined account order (ids in display order).
pub async fn reorder(pool: &SqlitePool, ids: &[i64]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for (pos, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE accounts SET sort_order = ? WHERE id = ?")
            .bind(pos as i64 + 1)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn find_by_email(pool: &SqlitePool, kind: &str, email: &str) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar("SELECT id FROM accounts WHERE kind = ? AND lower(email) = lower(?)")
        .bind(kind)
        .bind(email)
        .fetch_optional(pool)
        .await?)
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM accounts WHERE id = ?").bind(id).execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::open_memory;

    fn config(email: &str, sender: &str) -> AccountConfig {
        AccountConfig {
            kind: "imap".into(),
            display_name: format!("Konto {email}"),
            sender_name: sender.into(),
            email: email.into(),
            imap_host: "h".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            smtp_host: "h".into(),
            smtp_port: 465,
            smtp_security: "tls".into(),
            username: email.into(),
        }
    }

    #[tokio::test]
    async fn keeps_names_separate_and_order_user_defined() {
        let pool = open_memory().await;
        let a = insert(&pool, &config("a@x.de", "Anna A")).await.unwrap();
        let b = insert(&pool, &config("b@x.de", "")).await.unwrap();
        let c = insert(&pool, &config("c@x.de", "")).await.unwrap();

        let ids = |l: Vec<Account>| l.into_iter().map(|a| a.id).collect::<Vec<_>>();
        assert_eq!(ids(list(&pool).await.unwrap()), vec![a, b, c], "new accounts go last");

        reorder(&pool, &[c, a, b]).await.unwrap();
        assert_eq!(ids(list(&pool).await.unwrap()), vec![c, a, b]);

        let acc = get(&pool, a).await.unwrap();
        assert_eq!((acc.display_name.as_str(), acc.sender_name.as_str()), ("Konto a@x.de", "Anna A"));
        update_profile(&pool, a, "Praxis", "Dr. Anna A", None).await.unwrap();
        let acc = get(&pool, a).await.unwrap();
        assert_eq!((acc.display_name.as_str(), acc.sender_name.as_str()), ("Praxis", "Dr. Anna A"));
    }
}
