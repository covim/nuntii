mod auth;
mod commands;
mod error;
mod html;
mod mime;
mod provider;
mod search;
mod state;
mod store;
mod sync;

use tauri::Manager;

use crate::search::SearchIndex;
use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nuntii_lib=info,warn".into()),
        )
        .init();
    let _ = rustls::crypto::ring::default_provider().install_default();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let handle = app.handle().clone();
            let state = tauri::async_runtime::block_on(async move {
                let db = store::open(&data_dir.join("nuntii.db")).await?;
                let search = SearchIndex::open(&data_dir.join("index"))?;
                search.spawn_committer();
                Ok::<_, error::AppError>(AppState::new(db, search, handle))
            })?;
            app.manage(state.clone());
            tauri::async_runtime::spawn(async move {
                if let Err(e) = state.start_all().await {
                    tracing::error!("Konten konnten nicht gestartet werden: {e}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::accounts::accounts_list,
            commands::accounts::account_test,
            commands::accounts::account_add_imap,
            commands::accounts::account_add_gmail,
            commands::accounts::account_update,
            commands::accounts::accounts_reorder,
            commands::accounts::account_update_password,
            commands::accounts::account_reconnect,
            commands::accounts::account_remove,
            commands::accounts::sync_now,
            commands::messages::folders_list,
            commands::messages::messages_list,
            commands::messages::thread_get,
            commands::messages::message_get,
            commands::messages::sender_trust,
            commands::messages::messages_set_flag,
            commands::messages::messages_move,
            commands::messages::messages_delete,
            commands::messages::attachment_save,
            commands::messages::search,
            commands::compose::compose_send,
            commands::compose::outbox_list,
            commands::compose::outbox_discard,
            commands::compose::drafts_list,
            commands::compose::draft_get,
            commands::compose::draft_save,
            commands::compose::draft_delete,
            commands::settings::settings_get,
            commands::settings::settings_set_gmail,
            commands::settings::settings_set_sync_window,
            commands::settings::signatures_list,
            commands::settings::signature_save,
            commands::settings::signature_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
