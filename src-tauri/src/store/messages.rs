use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::error::{AppError, Result};
use crate::mime::parse::{normalize_subject, EmailAddress, ParsedAttachment, ParsedHeader};
use crate::provider::Flags;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageRow {
    pub id: i64,
    pub account_id: i64,
    pub folder_id: i64,
    pub uid: i64,
    pub message_id_hdr: Option<String>,
    pub references_hdr: Option<String>,
    pub thread_id: Option<i64>,
    pub subject: String,
    pub from_json: String,
    pub to_json: String,
    pub cc_json: String,
    pub date: i64,
    pub snippet: String,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub has_attachments: bool,
    pub body_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: i64,
    pub account_id: i64,
    pub folder_id: i64,
    pub thread_id: Option<i64>,
    pub subject: String,
    pub from: Vec<EmailAddress>,
    pub to: Vec<EmailAddress>,
    pub date: i64,
    pub snippet: String,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub has_attachments: bool,
    pub body_fetched: bool,
}

impl MessageRow {
    pub fn sender_addrs(&self) -> Vec<EmailAddress> {
        serde_json::from_str(&self.from_json).unwrap_or_default()
    }
    pub fn to_addrs(&self) -> Vec<EmailAddress> {
        serde_json::from_str(&self.to_json).unwrap_or_default()
    }
    pub fn cc_addrs(&self) -> Vec<EmailAddress> {
        serde_json::from_str(&self.cc_json).unwrap_or_default()
    }
    pub fn references(&self) -> Vec<String> {
        self.references_hdr
            .as_deref()
            .map(|r| r.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default()
    }

    pub fn summary(&self) -> MessageSummary {
        MessageSummary {
            id: self.id,
            account_id: self.account_id,
            folder_id: self.folder_id,
            thread_id: self.thread_id,
            subject: self.subject.clone(),
            from: self.sender_addrs(),
            to: self.to_addrs(),
            date: self.date,
            snippet: self.snippet.clone(),
            seen: self.seen,
            flagged: self.flagged,
            answered: self.answered,
            has_attachments: self.has_attachments,
            body_fetched: self.body_state == "fetched",
        }
    }
}

const COLUMNS: &str = "id, account_id, folder_id, uid, message_id_hdr, references_hdr, thread_id, subject,
    from_json, to_json, cc_json, date, snippet, seen, flagged, answered, has_attachments, body_state";

pub struct NewHeader<'a> {
    pub account_id: i64,
    pub folder_id: i64,
    pub uid: u32,
    pub header: &'a ParsedHeader,
    pub flags: Flags,
    pub size: u32,
    pub internal_date: Option<i64>,
}

/// Finds the thread for a message via its Message-ID/References/In-Reply-To (simplified JWZ),
/// creating a new thread when nothing links it to known mail.
async fn resolve_thread(tx: &mut Transaction<'_, Sqlite>, account_id: i64, h: &ParsedHeader) -> Result<i64> {
    let mut candidates: Vec<&str> = Vec::new();
    if let Some(id) = &h.message_id {
        candidates.push(id);
    }
    if let Some(irt) = &h.in_reply_to {
        candidates.push(irt);
    }
    candidates.extend(h.references.iter().rev().map(String::as_str));

    for c in &candidates {
        let found: Option<i64> =
            sqlx::query_scalar("SELECT thread_id FROM thread_refs WHERE account_id = ? AND message_id_hdr = ?")
                .bind(account_id)
                .bind(c)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some(t) = found {
            return Ok(t);
        }
    }
    Ok(sqlx::query("INSERT INTO threads (account_id, subject_norm) VALUES (?, ?)")
        .bind(account_id)
        .bind(normalize_subject(&h.subject))
        .execute(&mut **tx)
        .await?
        .last_insert_rowid())
}

/// Inserts a newly synced header (or refreshes flags if the UID is already known). Returns the row id.
pub async fn upsert_header(pool: &SqlitePool, n: NewHeader<'_>) -> Result<i64> {
    let mut tx = pool.begin().await?;

    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM messages WHERE folder_id = ? AND uid = ?")
        .bind(n.folder_id)
        .bind(n.uid as i64)
        .fetch_optional(&mut *tx)
        .await?;
    if let Some(id) = existing {
        tx.commit().await?;
        set_flags(pool, id, n.flags).await?;
        return Ok(id);
    }

    let h = n.header;
    let thread_id = resolve_thread(&mut tx, n.account_id, h).await?;
    let json = |v: &Vec<EmailAddress>| serde_json::to_string(v).unwrap_or_else(|_| "[]".into());
    let date = h.date.or(n.internal_date).unwrap_or_else(super::now);

    let id = sqlx::query(
        "INSERT INTO messages (account_id, folder_id, uid, message_id_hdr, in_reply_to, references_hdr, thread_id,
            subject, from_json, to_json, cc_json, date, seen, flagged, answered, draft, has_attachments, size)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(n.account_id)
    .bind(n.folder_id)
    .bind(n.uid as i64)
    .bind(&h.message_id)
    .bind(&h.in_reply_to)
    .bind((!h.references.is_empty()).then(|| h.references.join(" ")))
    .bind(thread_id)
    .bind(&h.subject)
    .bind(json(&h.from))
    .bind(json(&h.to))
    .bind(json(&h.cc))
    .bind(date)
    .bind(n.flags.seen)
    .bind(n.flags.flagged)
    .bind(n.flags.answered)
    .bind(n.flags.draft)
    .bind(h.has_attachments_hint)
    .bind(n.size as i64)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();

    let mut refs: Vec<&str> = h.references.iter().map(String::as_str).collect();
    refs.extend(h.message_id.as_deref());
    refs.extend(h.in_reply_to.as_deref());
    for r in refs {
        sqlx::query("INSERT OR IGNORE INTO thread_refs (account_id, message_id_hdr, thread_id) VALUES (?, ?, ?)")
            .bind(n.account_id)
            .bind(r)
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(id)
}

pub async fn set_flags(pool: &SqlitePool, id: i64, f: Flags) -> Result<()> {
    sqlx::query("UPDATE messages SET seen = ?, flagged = ?, answered = ?, draft = ? WHERE id = ?")
        .bind(f.seen)
        .bind(f.flagged)
        .bind(f.answered)
        .bind(f.draft)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_flags_by_uid(pool: &SqlitePool, folder_id: i64, uid: u32, f: Flags) -> Result<()> {
    sqlx::query("UPDATE messages SET seen = ?, flagged = ?, answered = ?, draft = ? WHERE folder_id = ? AND uid = ?")
        .bind(f.seen)
        .bind(f.flagged)
        .bind(f.answered)
        .bind(f.draft)
        .bind(folder_id)
        .bind(uid as i64)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_bool(pool: &SqlitePool, ids: &[i64], column: &str, value: bool) -> Result<()> {
    let column = match column {
        "seen" | "flagged" | "answered" => column,
        _ => return Err(AppError::Invalid(format!("Unbekanntes Flag {column}"))),
    };
    for id in ids {
        sqlx::query(&format!("UPDATE messages SET {column} = ? WHERE id = ?"))
            .bind(value)
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<MessageRow> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM messages WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Nachricht {id}")))
}

pub async fn get_many(pool: &SqlitePool, ids: &[i64]) -> Result<Vec<MessageRow>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Ok(row) = get(pool, *id).await {
            out.push(row);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFilter {
    All,
    Unread,
    Flagged,
}

impl ListFilter {
    pub fn parse(s: &str) -> Self {
        match s {
            "unread" => ListFilter::Unread,
            "flagged" => ListFilter::Flagged,
            _ => ListFilter::All,
        }
    }
}

/// Lists a folder, newest first. `keep_id` stays in the result even if it no longer matches the
/// filter, so the open message does not vanish when reading marks it as seen.
pub async fn list_folder(
    pool: &SqlitePool,
    folder_id: i64,
    filter: ListFilter,
    keep_id: Option<i64>,
    offset: i64,
    limit: i64,
) -> Result<Vec<MessageRow>> {
    let condition = match filter {
        ListFilter::All => "1 = 1",
        ListFilter::Unread => "seen = 0",
        ListFilter::Flagged => "flagged = 1",
    };
    Ok(sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM messages WHERE folder_id = ? AND ({condition} OR id = ?)
         ORDER BY date DESC, id DESC LIMIT ? OFFSET ?"
    ))
    .bind(folder_id)
    .bind(keep_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?)
}

pub async fn list_thread(pool: &SqlitePool, thread_id: i64) -> Result<Vec<MessageRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM messages WHERE thread_id = ? ORDER BY date ASC, id ASC"
    ))
    .bind(thread_id)
    .fetch_all(pool)
    .await?)
}

pub async fn local_uids(pool: &SqlitePool, folder_id: i64) -> Result<Vec<(i64, u32)>> {
    let rows: Vec<(i64, i64)> = sqlx::query_as("SELECT id, uid FROM messages WHERE folder_id = ?")
        .bind(folder_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|(id, uid)| (id, uid as u32)).collect())
}

pub async fn delete(pool: &SqlitePool, ids: &[i64]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for id in ids {
        sqlx::query("DELETE FROM messages WHERE id = ?").bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn delete_folder_messages(pool: &SqlitePool, folder_id: i64) -> Result<Vec<i64>> {
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM messages WHERE folder_id = ?")
        .bind(folder_id)
        .fetch_all(pool)
        .await?;
    sqlx::query("DELETE FROM messages WHERE folder_id = ?").bind(folder_id).execute(pool).await?;
    Ok(ids)
}

/// Messages without a downloaded body, newest first (for background prefetch).
pub async fn needing_body(pool: &SqlitePool, folder_id: i64, since: i64, limit: i64) -> Result<Vec<(i64, u32)>> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT id, uid FROM messages WHERE folder_id = ? AND body_state = 'none' AND date >= ? ORDER BY date DESC LIMIT ?",
    )
    .bind(folder_id)
    .bind(since)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id, uid)| (id, uid as u32)).collect())
}

pub async fn store_body(
    pool: &SqlitePool,
    id: i64,
    raw: &[u8],
    text: &str,
    snippet: &str,
    attachments: &[ParsedAttachment],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT OR REPLACE INTO message_bodies (message_id, raw, text_plain) VALUES (?, ?, ?)")
        .bind(id)
        .bind(raw)
        .bind(text)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM attachments WHERE message_id = ?").bind(id).execute(&mut *tx).await?;
    for a in attachments {
        sqlx::query(
            "INSERT INTO attachments (message_id, idx, filename, mime, size, content_id, inline) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(a.idx as i64)
        .bind(&a.filename)
        .bind(&a.mime)
        .bind(a.size as i64)
        .bind(&a.content_id)
        .bind(a.inline)
        .execute(&mut *tx)
        .await?;
    }
    let visible_attachments = attachments.iter().any(|a| !a.inline || a.content_id.is_none());
    sqlx::query("UPDATE messages SET body_state = 'fetched', snippet = ?, has_attachments = ? WHERE id = ?")
        .bind(snippet)
        .bind(visible_attachments)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn raw_body(pool: &SqlitePool, id: i64) -> Result<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar("SELECT raw FROM message_bodies WHERE message_id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub id: i64,
    pub idx: i64,
    pub filename: String,
    pub mime: String,
    pub size: i64,
    pub content_id: Option<String>,
    pub inline: bool,
}

pub async fn attachments(pool: &SqlitePool, message_id: i64) -> Result<Vec<AttachmentInfo>> {
    Ok(sqlx::query_as(
        "SELECT id, idx, filename, mime, size, content_id, inline FROM attachments WHERE message_id = ? ORDER BY idx",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{accounts, folders, open_memory};
    use crate::provider::{FolderRole, RemoteFolder};

    async fn setup() -> (SqlitePool, i64, i64) {
        let pool = open_memory().await;
        let acc = accounts::insert(
            &pool,
            &accounts::AccountConfig {
                kind: "imap".into(),
                display_name: "Test".into(),
                sender_name: String::new(),
                email: "me@example.de".into(),
                imap_host: "imap.example.de".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                smtp_host: "smtp.example.de".into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                username: "me".into(),
            },
        )
        .await
        .unwrap();
        folders::reconcile(
            &pool,
            acc,
            &[RemoteFolder { name: "INBOX".into(), delimiter: Some("/".into()), role: Some(FolderRole::Inbox), selectable: true }],
        )
        .await
        .unwrap();
        let folder = folders::list(&pool, Some(acc)).await.unwrap()[0].id;
        (pool, acc, folder)
    }

    fn header(id: &str, irt: Option<&str>, refs: &[&str]) -> ParsedHeader {
        ParsedHeader {
            subject: "Termin".into(),
            message_id: Some(id.into()),
            in_reply_to: irt.map(str::to_string),
            references: refs.iter().map(|s| s.to_string()).collect(),
            date: Some(1_700_000_000),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn threads_replies_together() {
        let (pool, acc, folder) = setup().await;
        let h1 = header("a@x", None, &[]);
        let h2 = header("b@x", Some("a@x"), &["a@x"]);
        let h3 = header("c@x", None, &[]);
        let ins = |uid, h| NewHeader { account_id: acc, folder_id: folder, uid, header: h, flags: Flags::default(), size: 10, internal_date: None };
        let m1 = upsert_header(&pool, ins(1, &h1)).await.unwrap();
        let m2 = upsert_header(&pool, ins(2, &h2)).await.unwrap();
        let m3 = upsert_header(&pool, ins(3, &h3)).await.unwrap();
        let t1 = get(&pool, m1).await.unwrap().thread_id;
        assert_eq!(t1, get(&pool, m2).await.unwrap().thread_id);
        assert_ne!(t1, get(&pool, m3).await.unwrap().thread_id);
        assert_eq!(list_thread(&pool, t1.unwrap()).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn upsert_is_idempotent_and_updates_flags() {
        let (pool, acc, folder) = setup().await;
        let h = header("a@x", None, &[]);
        let id1 = upsert_header(&pool, NewHeader { account_id: acc, folder_id: folder, uid: 7, header: &h, flags: Flags::default(), size: 1, internal_date: None }).await.unwrap();
        let seen = Flags { seen: true, ..Default::default() };
        let id2 = upsert_header(&pool, NewHeader { account_id: acc, folder_id: folder, uid: 7, header: &h, flags: seen, size: 1, internal_date: None }).await.unwrap();
        assert_eq!(id1, id2);
        assert!(get(&pool, id1).await.unwrap().seen);
        let f = folders::get(&pool, folder).await.unwrap();
        assert_eq!((f.total_count, f.unread_count), (1, 0));
    }

    #[tokio::test]
    async fn filters_unread_but_keeps_open_message() {
        let (pool, acc, folder) = setup().await;
        let (a, b) = (header("a@x", None, &[]), header("b@x", None, &[]));
        let read = Flags { seen: true, ..Default::default() };
        let seen_id = upsert_header(&pool, NewHeader { account_id: acc, folder_id: folder, uid: 1, header: &a, flags: read, size: 1, internal_date: None }).await.unwrap();
        let unread_id = upsert_header(&pool, NewHeader { account_id: acc, folder_id: folder, uid: 2, header: &b, flags: Flags::default(), size: 1, internal_date: None }).await.unwrap();

        let ids = |rows: Vec<MessageRow>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(list_folder(&pool, folder, ListFilter::All, None, 0, 10).await.unwrap()).len(), 2);
        assert_eq!(ids(list_folder(&pool, folder, ListFilter::Unread, None, 0, 10).await.unwrap()), vec![unread_id]);
        let kept = ids(list_folder(&pool, folder, ListFilter::Unread, Some(seen_id), 0, 10).await.unwrap());
        assert!(kept.contains(&seen_id) && kept.contains(&unread_id));
        assert!(list_folder(&pool, folder, ListFilter::Flagged, None, 0, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn stores_body_for_offline_reading() {
        let (pool, acc, folder) = setup().await;
        let h = header("a@x", None, &[]);
        let id = upsert_header(&pool, NewHeader { account_id: acc, folder_id: folder, uid: 1, header: &h, flags: Flags::default(), size: 1, internal_date: None }).await.unwrap();
        assert_eq!(needing_body(&pool, folder, 0, 10).await.unwrap().len(), 1);
        store_body(&pool, id, b"raw", "text", "snip", &[]).await.unwrap();
        assert!(needing_body(&pool, folder, 0, 10).await.unwrap().is_empty());
        assert_eq!(raw_body(&pool, id).await.unwrap().unwrap(), b"raw");
        assert_eq!(get(&pool, id).await.unwrap().snippet, "snip");
    }
}
