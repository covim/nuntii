use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, Result};
use crate::mime::build::{build, Draft, OutgoingAttachment};
use crate::mime::parse::{attachment_bytes, parse_message};
use crate::state::AppState;
use crate::store::drafts::{self, Draft as StoredDraft};
use crate::store::{self, accounts, messages};
use crate::sync::engine::{emit_changed, ensure_body, send_outbox_item};

type AppStateRef<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeInput {
    pub account_id: i64,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    /// Formatted version from the rich-text editor (plain `body_text` is the fallback part).
    pub body_html: Option<String>,
    pub reply_to_message_id: Option<i64>,
    pub forward_message_id: Option<i64>,
    /// Indices (mail-parser attachment positions) of the forwarded message's attachments to include.
    pub forward_attachment_idx: Vec<usize>,
    pub attachment_paths: Vec<String>,
    /// Auto-saved draft to remove once the mail is queued.
    pub draft_id: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    /// false: the mail is queued in the outbox and will be retried automatically.
    pub sent: bool,
    pub error: Option<String>,
}

async fn read_attachment(path: &str) -> Result<OutgoingAttachment> {
    let p = Path::new(path);
    let data = tokio::fs::read(p).await?;
    let filename = p
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "anhang".into());
    let mime = mime_guess::from_path(p).first_or_octet_stream().to_string();
    Ok(OutgoingAttachment { filename, mime, data })
}

#[tauri::command]
pub async fn compose_send(state: AppStateRef<'_>, input: ComposeInput) -> Result<SendResult> {
    let account = accounts::get(&state.db, input.account_id).await?;
    let from = lettre::message::Mailbox::new(
        Some(account.sender_name.trim().to_string()).filter(|n| !n.is_empty()),
        account
            .email
            .parse()
            .map_err(|_| AppError::Invalid(format!("Ungültige Absenderadresse {}", account.email)))?,
    );

    let (mut in_reply_to, mut references) = (None, Vec::new());
    if let Some(orig_id) = input.reply_to_message_id {
        let orig = messages::get(&state.db, orig_id).await?;
        references = orig.references();
        if let Some(mid) = &orig.message_id_hdr {
            references.push(mid.clone());
            in_reply_to = Some(mid.clone());
        }
    }

    let mut attachments = Vec::new();
    if let Some(fwd_id) = input.forward_message_id {
        let orig = messages::get(&state.db, fwd_id).await?;
        let raw = ensure_body(&state, &orig).await?;
        let parsed = parse_message(&raw);
        for idx in &input.forward_attachment_idx {
            if let Some(a) = parsed.attachments.iter().find(|a| a.idx == *idx) {
                if let Some(data) = attachment_bytes(&raw, a.idx) {
                    attachments.push(OutgoingAttachment {
                        filename: a.filename.clone(),
                        mime: a.mime.clone(),
                        data,
                    });
                }
            }
        }
    }
    for path in &input.attachment_paths {
        attachments.push(read_attachment(path).await?);
    }

    let built = build(Draft {
        from,
        to: input.to,
        cc: input.cc,
        bcc: input.bcc,
        subject: input.subject,
        body_text: input.body_text,
        body_html: input.body_html.filter(|h| !h.trim().is_empty()),
        in_reply_to,
        references,
        attachments,
    })?;

    // Queue first so nothing is lost if sending fails or the app closes mid-send.
    let outbox_id = store::outbox_add(
        &state.db,
        account.id,
        &built.raw,
        &built.env_from,
        &built.env_to,
        input.reply_to_message_id,
    )
    .await?;
    // The mail is safe in the outbox now; the draft is no longer needed.
    if let Some(draft_id) = input.draft_id {
        drafts::delete(&state.db, draft_id).await?;
    }
    let item = store::outbox_get(&state.db, outbox_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Postausgang".into()))?;
    let provider = state.provider(account.id).await?;
    let result = send_outbox_item(&state, &provider, &item).await;
    emit_changed(&state, account.id, Vec::new());
    state.wake(Some(account.id)).await;

    Ok(match result {
        Ok(()) => SendResult { sent: true, error: None },
        Err(e) => SendResult { sent: false, error: Some(e.to_string()) },
    })
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OutboxEntry {
    pub id: i64,
    pub account_id: i64,
    pub status: String,
    pub error: Option<String>,
    pub created_at: i64,
}

#[tauri::command]
pub async fn outbox_list(state: AppStateRef<'_>) -> Result<Vec<OutboxEntry>> {
    Ok(sqlx::query_as("SELECT id, account_id, status, error, created_at FROM outbox ORDER BY id")
        .fetch_all(&state.db)
        .await?)
}

#[tauri::command]
pub async fn outbox_discard(state: AppStateRef<'_>, id: i64) -> Result<()> {
    store::outbox_done(&state.db, id).await
}

#[tauri::command]
pub async fn drafts_list(state: AppStateRef<'_>) -> Result<Vec<StoredDraft>> {
    drafts::list(&state.db).await
}

#[tauri::command]
pub async fn draft_get(state: AppStateRef<'_>, id: i64) -> Result<StoredDraft> {
    drafts::get(&state.db, id).await
}

/// Auto-save from the compose window. Returns the draft id (assigned on first save).
#[tauri::command]
pub async fn draft_save(state: AppStateRef<'_>, draft: StoredDraft) -> Result<i64> {
    drafts::save(&state.db, &draft).await
}

#[tauri::command]
pub async fn draft_delete(state: AppStateRef<'_>, id: i64) -> Result<()> {
    drafts::delete(&state.db, id).await
}
