use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::auth::keychain;
use crate::error::{AppError, Result};
use crate::state::{AppState, SETTING_GMAIL_CLIENT_ID};
use crate::store::{self, Signature};
use crate::sync::engine::{DEFAULT_WINDOW_DAYS, SETTING_WINDOW_DAYS};

type AppStateRef<'a> = State<'a, Arc<AppState>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub gmail_client_id: String,
    pub gmail_has_secret: bool,
    pub sync_window_days: i64,
}

#[tauri::command]
pub async fn settings_get(state: AppStateRef<'_>) -> Result<Settings> {
    Ok(Settings {
        gmail_client_id: store::setting(&state.db, SETTING_GMAIL_CLIENT_ID).await?.unwrap_or_default(),
        gmail_has_secret: keychain::get(keychain::GMAIL_CLIENT_SECRET_KEY)?.is_some(),
        sync_window_days: store::setting(&state.db, SETTING_WINDOW_DAYS)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_WINDOW_DAYS),
    })
}

/// Stores the Google OAuth client. The secret goes to the keychain; empty keeps the old one.
#[tauri::command]
pub async fn settings_set_gmail(state: AppStateRef<'_>, client_id: String, client_secret: String) -> Result<()> {
    store::set_setting(&state.db, SETTING_GMAIL_CLIENT_ID, client_id.trim()).await?;
    if !client_secret.trim().is_empty() {
        keychain::set(keychain::GMAIL_CLIENT_SECRET_KEY, client_secret.trim())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn settings_set_sync_window(state: AppStateRef<'_>, days: i64) -> Result<()> {
    if !(1..=3650).contains(&days) {
        return Err(AppError::Invalid("Zeitraum muss zwischen 1 und 3650 Tagen liegen".into()));
    }
    store::set_setting(&state.db, SETTING_WINDOW_DAYS, &days.to_string()).await
}

#[tauri::command]
pub async fn signatures_list(state: AppStateRef<'_>) -> Result<Vec<Signature>> {
    store::signatures(&state.db).await
}

#[tauri::command]
pub async fn signature_save(state: AppStateRef<'_>, id: Option<i64>, name: String, body_text: String) -> Result<i64> {
    if name.trim().is_empty() {
        return Err(AppError::Invalid("Name der Signatur fehlt".into()));
    }
    store::save_signature(&state.db, id, name.trim(), &body_text).await
}

#[tauri::command]
pub async fn signature_delete(state: AppStateRef<'_>, id: i64) -> Result<()> {
    store::delete_signature(&state.db, id).await
}
