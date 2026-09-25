use std::sync::Arc;

use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::auth::{keychain, oauth};
use crate::error::{AppError, Result};
use crate::provider::imap::ImapSmtpProvider;
use crate::provider::{Auth, MailProvider, ProviderKind};
use crate::state::{gmail_client_config, server_configs, AppState};
use crate::store::accounts::{self, Account, AccountConfig};

type AppStateRef<'a> = State<'a, Arc<AppState>>;

fn validate(c: &AccountConfig) -> Result<()> {
    let bad = |m: &str| Err(AppError::Invalid(m.into()));
    if !c.email.contains('@') {
        return bad("Bitte eine gültige E-Mail-Adresse angeben");
    }
    if c.imap_host.trim().is_empty() || c.smtp_host.trim().is_empty() {
        return bad("IMAP- und SMTP-Server sind erforderlich");
    }
    for s in [&c.imap_security, &c.smtp_security] {
        if s != "tls" && s != "starttls" {
            return bad("Nur verschlüsselte Verbindungen (TLS/STARTTLS) sind erlaubt");
        }
    }
    if !(1..=65535).contains(&c.imap_port) || !(1..=65535).contains(&c.smtp_port) {
        return bad("Ungültiger Port");
    }
    Ok(())
}

fn as_account(c: &AccountConfig) -> Account {
    Account {
        id: 0,
        kind: c.kind.clone(),
        display_name: c.display_name.clone(),
        sender_name: c.sender_name.clone(),
        sort_order: 0,
        email: c.email.clone(),
        imap_host: c.imap_host.trim().to_string(),
        imap_port: c.imap_port,
        imap_security: c.imap_security.clone(),
        smtp_host: c.smtp_host.trim().to_string(),
        smtp_port: c.smtp_port,
        smtp_security: c.smtp_security.clone(),
        username: c.username.clone(),
        signature_id: None,
    }
}

async fn test_imap(config: &AccountConfig, password: &str) -> Result<()> {
    validate(config)?;
    let (imap, smtp) = server_configs(&as_account(config));
    let provider = ImapSmtpProvider::new(
        ProviderKind::Imap,
        imap,
        smtp,
        Auth::Password {
            username: config.username.clone(),
            password: password.to_string(),
        },
    );
    provider.test().await
}

#[tauri::command]
pub async fn accounts_list(state: AppStateRef<'_>) -> Result<Vec<Account>> {
    accounts::list(&state.db).await
}

#[tauri::command]
pub async fn account_test(config: AccountConfig, password: String) -> Result<()> {
    test_imap(&config, &password).await
}

#[tauri::command]
pub async fn account_add_imap(state: AppStateRef<'_>, mut config: AccountConfig, password: String) -> Result<Account> {
    config.kind = "imap".into();
    test_imap(&config, &password).await?;
    let id = accounts::insert(&state.db, &config).await?;
    if let Err(e) = keychain::set(&keychain::account_password_key(id), &password) {
        accounts::delete(&state.db, id).await?;
        return Err(e);
    }
    let account = accounts::get(&state.db, id).await?;
    state.start_account(&account, None).await?;
    Ok(account)
}

/// Runs the Google OAuth flow in the system browser and adds (or re-authorises) a Gmail account.
#[tauri::command]
pub async fn account_add_gmail(state: AppStateRef<'_>, sender_name: Option<String>) -> Result<Account> {
    let cfg = gmail_client_config(&state.db).await?;
    let app = state.app.clone();
    let login = oauth::google_login(&cfg, |url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AppError::OAuth(format!("Browser konnte nicht geöffnet werden: {e}")))
    })
    .await?;

    let id = match accounts::find_by_email(&state.db, "gmail", &login.email).await? {
        Some(existing) => existing,
        None => {
            accounts::insert(
                &state.db,
                &AccountConfig {
                    kind: "gmail".into(),
                    display_name: login.email.clone(),
                    sender_name: sender_name.unwrap_or_default(),
                    email: login.email.clone(),
                    imap_host: "imap.gmail.com".into(),
                    imap_port: 993,
                    imap_security: "tls".into(),
                    smtp_host: "smtp.gmail.com".into(),
                    smtp_port: 465,
                    smtp_security: "tls".into(),
                    username: login.email.clone(),
                },
            )
            .await?
        }
    };
    keychain::set(&keychain::account_refresh_token_key(id), &login.refresh_token)?;
    let account = accounts::get(&state.db, id).await?;
    state
        .start_account(&account, Some((login.access_token, login.expires_in)))
        .await?;
    Ok(account)
}

#[tauri::command]
pub async fn account_update(
    state: AppStateRef<'_>,
    id: i64,
    display_name: String,
    sender_name: String,
    signature_id: Option<i64>,
) -> Result<Account> {
    let account = accounts::get(&state.db, id).await?;
    let display_name = Some(display_name.trim()).filter(|n| !n.is_empty()).unwrap_or(&account.email);
    accounts::update_profile(&state.db, id, display_name, sender_name.trim(), signature_id).await?;
    accounts::get(&state.db, id).await
}

/// Stores the account order shown in the sidebar (ids in display order).
#[tauri::command]
pub async fn accounts_reorder(state: AppStateRef<'_>, ids: Vec<i64>) -> Result<()> {
    accounts::reorder(&state.db, &ids).await
}

#[tauri::command]
pub async fn account_remove(state: AppStateRef<'_>, id: i64) -> Result<()> {
    state.stop_account(id).await;
    accounts::delete(&state.db, id).await?;
    state.search.remove_account(id);
    keychain::delete(&keychain::account_password_key(id))?;
    keychain::delete(&keychain::account_refresh_token_key(id))?;
    Ok(())
}

#[tauri::command]
pub async fn sync_now(state: AppStateRef<'_>, account_id: Option<i64>) -> Result<()> {
    state.wake(account_id).await;
    Ok(())
}

/// Restarts an account's sync loop, e.g. after an authentication error was fixed.
#[tauri::command]
pub async fn account_reconnect(state: AppStateRef<'_>, id: i64) -> Result<()> {
    let account = accounts::get(&state.db, id).await?;
    state.start_account(&account, None).await
}

#[tauri::command]
pub async fn account_update_password(state: AppStateRef<'_>, id: i64, password: String) -> Result<()> {
    let account = accounts::get(&state.db, id).await?;
    if account.kind != "imap" {
        return Err(AppError::Invalid("Gmail-Konten verwenden OAuth – bitte neu verbinden".into()));
    }
    let config = AccountConfig {
        kind: account.kind.clone(),
        display_name: account.display_name.clone(),
        sender_name: account.sender_name.clone(),
        email: account.email.clone(),
        imap_host: account.imap_host.clone(),
        imap_port: account.imap_port,
        imap_security: account.imap_security.clone(),
        smtp_host: account.smtp_host.clone(),
        smtp_port: account.smtp_port,
        smtp_security: account.smtp_security.clone(),
        username: account.username.clone(),
    };
    test_imap(&config, &password).await?;
    keychain::set(&keychain::account_password_key(id), &password)?;
    state.start_account(&account, None).await
}
