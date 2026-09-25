use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use base64::Engine;
use serde::Serialize;
use tauri::State;

use crate::error::{AppError, Result};
use crate::html::sanitize::{sanitize, wrap_document};
use crate::mime::parse::{attachment_bytes, parse_message, EmailAddress};
use crate::provider::FlagKind;
use crate::state::AppState;
use crate::store::folders::{self, Folder};
use crate::store::messages::{self, AttachmentInfo, ListFilter, MessageRow, MessageSummary};
use crate::store;
use crate::sync::engine::{emit_changed, ensure_body};

type AppStateRef<'a> = State<'a, Arc<AppState>>;

const MAX_INLINE_IMAGE: usize = 5 * 1024 * 1024;

#[tauri::command]
pub async fn folders_list(state: AppStateRef<'_>, account_id: Option<i64>) -> Result<Vec<Folder>> {
    folders::list(&state.db, account_id).await
}

#[tauri::command]
pub async fn messages_list(
    state: AppStateRef<'_>,
    folder_id: i64,
    offset: i64,
    limit: i64,
    filter: Option<String>,
    keep_id: Option<i64>,
) -> Result<Vec<MessageSummary>> {
    let filter = ListFilter::parse(filter.as_deref().unwrap_or("all"));
    let rows = messages::list_folder(&state.db, folder_id, filter, keep_id, offset, limit.clamp(1, 500)).await?;
    Ok(rows.iter().map(MessageRow::summary).collect())
}

#[tauri::command]
pub async fn thread_get(state: AppStateRef<'_>, thread_id: i64) -> Result<Vec<MessageSummary>> {
    let rows = messages::list_thread(&state.db, thread_id).await?;
    Ok(rows.iter().map(MessageRow::summary).collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub cc: Vec<EmailAddress>,
    pub text: Option<String>,
    /// Complete sanitized HTML document for a sandboxed `srcdoc` iframe.
    pub html_document: Option<String>,
    pub blocked_remote: usize,
    pub remote_allowed: bool,
    pub attachments: Vec<AttachmentInfo>,
    pub message_id_hdr: Option<String>,
    pub references: Vec<String>,
}

#[tauri::command]
pub async fn message_get(state: AppStateRef<'_>, id: i64, allow_remote: bool) -> Result<MessageDetail> {
    let row = messages::get(&state.db, id).await?;
    let raw = ensure_body(&state, &row).await?;
    let row = messages::get(&state.db, id).await?; // snippet/attachment flags may have changed
    let parsed = parse_message(&raw);

    let sender = row.sender_addrs().first().map(|a| a.address.clone());
    let trusted = match &sender {
        Some(s) => store::is_trusted_sender(&state.db, s).await?,
        None => false,
    };
    let remote_allowed = allow_remote || trusted;

    let (html_document, blocked_remote) = match &parsed.html {
        Some(html) => {
            let mut inline: HashMap<String, String> = HashMap::new();
            for a in parsed.attachments.iter().filter(|a| a.mime.starts_with("image/")) {
                if let (Some(cid), true) = (&a.content_id, a.size <= MAX_INLINE_IMAGE) {
                    if let Some(bytes) = attachment_bytes(&raw, a.idx) {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                        inline.insert(cid.clone(), format!("data:{};base64,{b64}", a.mime));
                    }
                }
            }
            let s = sanitize(html, &inline, remote_allowed);
            (Some(wrap_document(&s.html, remote_allowed)), s.blocked_remote)
        }
        None => (None, 0),
    };

    Ok(MessageDetail {
        summary: row.summary(),
        cc: row.cc_addrs(),
        text: parsed.text,
        html_document,
        blocked_remote,
        remote_allowed,
        attachments: messages::attachments(&state.db, id).await?,
        message_id_hdr: row.message_id_hdr.clone(),
        references: row.references(),
    })
}

#[tauri::command]
pub async fn sender_trust(state: AppStateRef<'_>, address: String) -> Result<()> {
    store::trust_sender(&state.db, &address).await
}

/// Groups message rows by (account, folder) for server operations.
fn by_folder(rows: &[MessageRow]) -> BTreeMap<(i64, i64), Vec<&MessageRow>> {
    let mut map: BTreeMap<(i64, i64), Vec<&MessageRow>> = BTreeMap::new();
    for r in rows {
        map.entry((r.account_id, r.folder_id)).or_default().push(r);
    }
    map
}

#[tauri::command]
pub async fn messages_set_flag(state: AppStateRef<'_>, ids: Vec<i64>, flag: String, value: bool) -> Result<()> {
    let kind = match flag.as_str() {
        "seen" => FlagKind::Seen,
        "flagged" => FlagKind::Flagged,
        _ => return Err(AppError::Invalid(format!("Unbekanntes Flag {flag}"))),
    };
    let rows = messages::get_many(&state.db, &ids).await?;
    // Optimistic local update; rolled back if the server rejects it.
    messages::set_bool(&state.db, &ids, &flag, value).await?;

    let mut touched = Vec::new();
    for ((account_id, folder_id), group) in by_folder(&rows) {
        let result = async {
            let provider = state.provider(account_id).await?;
            let folder = folders::sync_state(&state.db, folder_id).await?;
            let uids: Vec<u32> = group.iter().map(|r| r.uid as u32).collect();
            provider.set_flag(&folder.remote_name, &uids, kind, value).await
        }
        .await;
        if let Err(e) = result {
            for r in &group {
                let previous = if flag == "seen" { r.seen } else { r.flagged };
                messages::set_bool(&state.db, &[r.id], &flag, previous).await?;
            }
            emit_changed(&state, account_id, vec![folder_id]);
            return Err(e);
        }
        touched.push((account_id, folder_id));
    }
    for (account_id, folder_id) in touched {
        emit_changed(&state, account_id, vec![folder_id]);
    }
    Ok(())
}

async fn move_rows(state: &AppState, rows: &[MessageRow], target: &folders::SyncState, target_id: i64) -> Result<()> {
    for ((account_id, folder_id), group) in by_folder(rows) {
        if folder_id == target_id {
            continue;
        }
        let provider = state.provider(account_id).await?;
        let source = folders::sync_state(&state.db, folder_id).await?;
        let uids: Vec<u32> = group.iter().map(|r| r.uid as u32).collect();
        provider.move_messages(&source.remote_name, &uids, &target.remote_name).await?;
        let ids: Vec<i64> = group.iter().map(|r| r.id).collect();
        messages::delete(&state.db, &ids).await?;
        state.search.remove(&ids);
        emit_changed(state, account_id, vec![folder_id, target_id]);
        state.wake(Some(account_id)).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn messages_move(state: AppStateRef<'_>, ids: Vec<i64>, target_folder_id: i64) -> Result<()> {
    let rows = messages::get_many(&state.db, &ids).await?;
    let target_folder = folders::get(&state.db, target_folder_id).await?;
    if rows.iter().any(|r| r.account_id != target_folder.account_id) {
        return Err(AppError::Invalid("Verschieben zwischen Konten wird nicht unterstützt".into()));
    }
    let target = folders::sync_state(&state.db, target_folder_id).await?;
    move_rows(&state, &rows, &target, target_folder_id).await
}

/// Moves to the account's trash; messages already in the trash are deleted permanently.
#[tauri::command]
pub async fn messages_delete(state: AppStateRef<'_>, ids: Vec<i64>) -> Result<()> {
    let rows = messages::get_many(&state.db, &ids).await?;
    for ((account_id, folder_id), group) in by_folder(&rows) {
        let group: Vec<MessageRow> = group.into_iter().cloned().collect();
        let trash = folders::by_role(&state.db, account_id, "trash").await?;
        match trash {
            Some(t) if t.id != folder_id => {
                let tid = t.id;
                move_rows(&state, &group, &t, tid).await?;
            }
            _ => {
                let provider = state.provider(account_id).await?;
                let source = folders::sync_state(&state.db, folder_id).await?;
                let uids: Vec<u32> = group.iter().map(|r| r.uid as u32).collect();
                provider.delete_permanently(&source.remote_name, &uids).await?;
                let ids: Vec<i64> = group.iter().map(|r| r.id).collect();
                messages::delete(&state.db, &ids).await?;
                state.search.remove(&ids);
                emit_changed(&state, account_id, vec![folder_id]);
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn attachment_save(state: AppStateRef<'_>, message_id: i64, idx: usize, path: String) -> Result<()> {
    let row = messages::get(&state.db, message_id).await?;
    let raw = ensure_body(&state, &row).await?;
    let bytes = attachment_bytes(&raw, idx).ok_or_else(|| AppError::NotFound("Anhang".into()))?;
    tokio::fs::write(&path, bytes).await?;
    Ok(())
}

#[tauri::command]
pub async fn search(state: AppStateRef<'_>, query: String, limit: Option<usize>) -> Result<Vec<MessageSummary>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let search = state.search.clone();
    let q = q.to_string();
    let limit = limit.unwrap_or(100).min(500);
    let ids = tokio::task::spawn_blocking(move || search.search(&q, limit))
        .await
        .map_err(|e| AppError::Invalid(e.to_string()))??;
    let rows = messages::get_many(&state.db, &ids).await?;
    Ok(rows.iter().map(MessageRow::summary).collect())
}
