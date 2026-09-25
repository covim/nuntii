use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use super::engine;
use crate::provider::MailProvider;
use crate::state::AppState;

const POLL_INTERVAL: Duration = Duration::from_secs(120);
const RETRY_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub account_id: i64,
    pub state: &'static str,
    pub error: Option<String>,
}

pub fn emit_status(app: &AppHandle, account_id: i64, state: &'static str, error: Option<String>) {
    let _ = app.emit("account://status", AccountStatus { account_id, state, error });
}

/// Background loop per account: sync, then sleep until the poll interval elapses or a
/// "sync now" is requested. Push (IMAP IDLE) is a phase-2 feature.
pub fn spawn(
    state: Arc<AppState>,
    account_id: i64,
    provider: Arc<dyn MailProvider>,
    wake: Arc<Notify>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        loop {
            emit_status(&state.app, account_id, "syncing", None);
            let delay = match engine::sync_account(&state, account_id, &provider).await {
                Ok(()) => {
                    emit_status(&state.app, account_id, "idle", None);
                    POLL_INTERVAL
                }
                Err(e) => {
                    tracing::warn!("Sync von Konto {account_id} fehlgeschlagen: {e}");
                    emit_status(&state.app, account_id, "error", Some(e.to_string()));
                    RETRY_INTERVAL
                }
            };
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = wake.notified() => {}
            }
        }
    })
}
