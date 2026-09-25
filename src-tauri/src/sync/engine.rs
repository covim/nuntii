//! Poll-based synchronisation of one account into the local cache, plus body download/ingest
//! shared with the reading-pane commands.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use tauri::Emitter;

use crate::error::{AppError, Result};
use crate::mime::parse::{html_to_text, parse_header, parse_message, snippet, EmailAddress};
use crate::provider::{FlagKind, MailProvider, OutgoingEnvelope};
use crate::search::SearchDoc;
use crate::state::AppState;
use crate::store::folders::SyncState;
use crate::store::messages::{self, MessageRow, NewHeader};
use crate::store::{self, folders};

const HEADER_BATCH: usize = 200;
const BODY_BATCH: usize = 10;
const PREFETCH_PER_CYCLE: i64 = 60;
const PREFETCH_DAYS: i64 = 30;
pub const DEFAULT_WINDOW_DAYS: i64 = 90;
pub const SETTING_WINDOW_DAYS: &str = "sync.window_days";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailChanged {
    pub account_id: i64,
    pub folder_ids: Vec<i64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub account_id: i64,
    pub folder: String,
    pub done: usize,
    pub total: usize,
}

pub fn emit_changed(state: &AppState, account_id: i64, folder_ids: Vec<i64>) {
    let _ = state.app.emit("mail://changed", MailChanged { account_id, folder_ids });
}

fn addr_text(list: &[EmailAddress]) -> String {
    list.iter()
        .map(|a| match &a.name {
            Some(n) => format!("{n} {}", a.address),
            None => a.address.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn index_row(state: &AppState, row: &MessageRow, body: &str) {
    let res = state.search.upsert(SearchDoc {
        msg_id: row.id,
        account_id: row.account_id,
        subject: &row.subject,
        from: &addr_text(&row.sender_addrs()),
        to: &addr_text(&[row.to_addrs(), row.cc_addrs()].concat()),
        body,
        date: row.date,
    });
    if let Err(e) = res {
        tracing::warn!("Indexierung von Nachricht {} fehlgeschlagen: {e}", row.id);
    }
}

/// Parses a downloaded message, stores it for offline reading and indexes its text.
pub async fn ingest_body(state: &AppState, row: &MessageRow, raw: &[u8]) -> Result<()> {
    let parsed = parse_message(raw);
    let text = parsed
        .text
        .clone()
        .or_else(|| parsed.html.as_deref().map(html_to_text))
        .unwrap_or_default();
    let snip = snippet(&text, 180);
    messages::store_body(&state.db, row.id, raw, &text, &snip, &parsed.attachments).await?;
    index_row(state, row, &text);
    Ok(())
}

/// Returns the raw message, downloading it first if it is not cached yet.
pub async fn ensure_body(state: &AppState, row: &MessageRow) -> Result<Vec<u8>> {
    if let Some(raw) = messages::raw_body(&state.db, row.id).await? {
        return Ok(raw);
    }
    let provider = state.provider(row.account_id).await?;
    let folder = folders::sync_state(&state.db, row.folder_id).await?;
    let fetched = provider
        .fetch_raw(&folder.remote_name, &[row.uid as u32])
        .await
        .map_err(|e| match e {
            AppError::Io(_) | AppError::Tls(_) | AppError::Imap(_) => AppError::Offline,
            other => other,
        })?;
    let (_, raw) = fetched
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Nachricht existiert auf dem Server nicht mehr".into()))?;
    ingest_body(state, row, &raw).await?;
    Ok(raw)
}

pub async fn window_start(state: &AppState) -> i64 {
    let days = store::setting(&state.db, SETTING_WINDOW_DAYS)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(DEFAULT_WINDOW_DAYS);
    store::now() - days * 86_400
}

/// Sends queued mail. Items stay in the outbox until the server accepted them.
pub async fn flush_outbox(state: &AppState, account_id: i64, provider: &Arc<dyn MailProvider>) -> Result<()> {
    for item in store::outbox_pending(&state.db, account_id).await? {
        send_outbox_item(state, provider, &item).await?;
    }
    Ok(())
}

pub async fn send_outbox_item(state: &AppState, provider: &Arc<dyn MailProvider>, item: &store::OutboxItem) -> Result<()> {
    let envelope = OutgoingEnvelope {
        from: item.env_from.clone(),
        to: serde_json::from_str(&item.env_to_json).unwrap_or_default(),
    };
    if let Err(e) = provider.send(&item.raw, &envelope).await {
        store::outbox_failed(&state.db, item.id, &e.to_string()).await?;
        return Err(e);
    }
    store::outbox_done(&state.db, item.id).await?;

    // Post-send bookkeeping is best effort: the mail is already out.
    if !provider.stores_sent_automatically() {
        if let Ok(Some(sent)) = folders::by_role(&state.db, item.account_id, "sent").await {
            if let Err(e) = provider.append(&sent.remote_name, &item.raw, true).await {
                tracing::warn!("Ablage in 'Gesendet' fehlgeschlagen: {e}");
            }
        }
    }
    if let Some(orig_id) = item.reply_to_message_id {
        if let Ok(orig) = messages::get(&state.db, orig_id).await {
            let _ = messages::set_bool(&state.db, &[orig_id], "answered", true).await;
            if let Ok(folder) = folders::sync_state(&state.db, orig.folder_id).await {
                let _ = provider
                    .set_flag(&folder.remote_name, &[orig.uid as u32], FlagKind::Answered, true)
                    .await;
            }
        }
    }
    Ok(())
}

pub async fn sync_account(state: &Arc<AppState>, account_id: i64, provider: &Arc<dyn MailProvider>) -> Result<()> {
    if let Err(e) = flush_outbox(state, account_id, provider).await {
        tracing::warn!("Postausgang konnte nicht gesendet werden: {e}");
    }

    let remote = provider.list_folders().await?;
    folders::reconcile(&state.db, account_id, &remote).await?;
    emit_changed(state, account_id, Vec::new());

    let since = window_start(state).await;
    for folder in folders::sync_states(&state.db, account_id).await? {
        if !folder.selectable || folder.role.as_deref() == Some("virtual") {
            continue;
        }
        match sync_folder(state, account_id, provider, &folder, since).await {
            Ok(true) => emit_changed(state, account_id, vec![folder.id]),
            Ok(false) => {}
            Err(e) => tracing::warn!("Ordner {} konnte nicht synchronisiert werden: {e}", folder.remote_name),
        }
    }

    // Background prefetch so recent mail can be read offline.
    let prefetch_since = store::now() - PREFETCH_DAYS * 86_400;
    for role in ["inbox", "sent"] {
        if let Some(folder) = folders::by_role(&state.db, account_id, role).await? {
            let pending = messages::needing_body(&state.db, folder.id, prefetch_since, PREFETCH_PER_CYCLE).await?;
            if pending.is_empty() {
                continue;
            }
            prefetch_bodies(state, provider, &folder, &pending).await?;
            emit_changed(state, account_id, vec![folder.id]);
        }
    }
    Ok(())
}

async fn prefetch_bodies(
    state: &AppState,
    provider: &Arc<dyn MailProvider>,
    folder: &SyncState,
    pending: &[(i64, u32)],
) -> Result<()> {
    let by_uid: HashMap<u32, i64> = pending.iter().map(|(id, uid)| (*uid, *id)).collect();
    for chunk in pending.chunks(BODY_BATCH) {
        let uids: Vec<u32> = chunk.iter().map(|(_, uid)| *uid).collect();
        for (uid, raw) in provider.fetch_raw(&folder.remote_name, &uids).await? {
            if let Some(id) = by_uid.get(&uid) {
                let row = messages::get(&state.db, *id).await?;
                ingest_body(state, &row, &raw).await?;
            }
        }
    }
    Ok(())
}

/// Synchronises one folder. Returns whether anything changed locally.
async fn sync_folder(
    state: &AppState,
    account_id: i64,
    provider: &Arc<dyn MailProvider>,
    folder: &SyncState,
    window_start: i64,
) -> Result<bool> {
    let name = folder.remote_name.as_str();
    let status = provider.folder_status(name).await?;
    let mut changed = false;
    let mut local_uidnext = folder.uidnext;
    let mut synced_since = folder.synced_since;

    if folder.uidvalidity.is_some_and(|v| v != status.uidvalidity as i64) {
        // UIDs were reassigned by the server: local state for this folder is meaningless.
        let removed = messages::delete_folder_messages(&state.db, folder.id).await?;
        state.search.remove(&removed);
        local_uidnext = None;
        synced_since = None;
        changed = true;
    }
    let first_sync = local_uidnext.is_none();
    // The sync window was enlarged (or never recorded): fetch the older mail it now covers.
    let needs_backfill = !first_sync && synced_since.is_none_or(|s| window_start < s);

    let unchanged = !first_sync
        && !needs_backfill
        && local_uidnext == Some(status.uidnext as i64)
        && folder.last_exists == Some(status.exists as i64)
        && status.highestmodseq.is_some()
        && folder.highestmodseq == status.highestmodseq.map(|m| m as i64);
    if unchanged {
        return Ok(changed);
    }

    let local: HashMap<u32, i64> = messages::local_uids(&state.db, folder.id)
        .await?
        .into_iter()
        .map(|(id, uid)| (uid, id))
        .collect();

    // 1. New messages.
    let mut new_uids: Vec<u32> = match local_uidnext {
        None => provider.uids_since(name, window_start).await?,
        Some(n) if status.uidnext as i64 > n || status.uidnext == 0 => provider.uids_from(name, n as u32).await?,
        Some(_) => Vec::new(),
    };
    if needs_backfill {
        new_uids.extend(provider.uids_since(name, window_start).await?);
    }
    new_uids.retain(|u| !local.contains_key(u));
    new_uids.sort_unstable_by(|a, b| b.cmp(a)); // newest first
    new_uids.dedup();
    let total = new_uids.len();
    for (i, chunk) in new_uids.chunks(HEADER_BATCH).enumerate() {
        for h in provider.fetch_headers(name, chunk).await? {
            let parsed = parse_header(&h.header);
            let id = messages::upsert_header(
                &state.db,
                NewHeader {
                    account_id,
                    folder_id: folder.id,
                    uid: h.uid,
                    header: &parsed,
                    flags: h.flags,
                    size: h.size,
                    internal_date: h.internal_date,
                },
            )
            .await?;
            let row = messages::get(&state.db, id).await?;
            index_row(state, &row, "");
        }
        changed = true;
        let _ = state.app.emit(
            "sync://progress",
            SyncProgress {
                account_id,
                folder: name.to_string(),
                done: ((i + 1) * HEADER_BATCH).min(total),
                total,
            },
        );
        emit_changed(state, account_id, vec![folder.id]);
    }

    if !first_sync && !local.is_empty() {
        // 2. Remote deletions / moves by other clients.
        if folder.last_exists != Some(status.exists as i64) || total > 0 {
            let remote: HashSet<u32> = provider.all_uids(name).await?.into_iter().collect();
            let gone: Vec<i64> = local
                .iter()
                .filter(|(uid, _)| !remote.contains(uid))
                .map(|(_, id)| *id)
                .collect();
            if !gone.is_empty() {
                messages::delete(&state.db, &gone).await?;
                state.search.remove(&gone);
                changed = true;
            }
        }

        // 3. Flag changes (only the delta when the server supports CONDSTORE).
        let min_uid = local.keys().copied().min().unwrap_or(1);
        let since_modseq = match (folder.highestmodseq, status.highestmodseq) {
            (Some(local_m), Some(remote_m)) if local_m as u64 == remote_m => None,
            (Some(local_m), Some(_)) => Some(Some(local_m as u64)),
            _ => Some(None),
        };
        if let Some(modseq) = since_modseq {
            for update in provider.fetch_flags(name, min_uid, modseq).await? {
                if local.contains_key(&update.uid) {
                    messages::set_flags_by_uid(&state.db, folder.id, update.uid, update.flags).await?;
                    changed = true;
                }
            }
        }
    }

    folders::update_sync_state(
        &state.db,
        folder.id,
        status.uidvalidity,
        status.uidnext,
        status.highestmodseq,
        status.exists,
        synced_since.map_or(window_start, |s| s.min(window_start)),
    )
    .await?;
    Ok(changed)
}
