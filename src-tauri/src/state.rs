use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::sync::{Notify, RwLock};

use crate::auth::keychain;
use crate::auth::oauth::{GoogleClientConfig, GoogleTokenManager};
use crate::error::{AppError, Result};
use crate::provider::imap::ImapSmtpProvider;
use crate::provider::{Auth, MailProvider, ProviderKind, Security, ServerConfig};
use crate::search::SearchIndex;
use crate::store::{self, accounts::Account};
use crate::sync;

pub const SETTING_GMAIL_CLIENT_ID: &str = "gmail.client_id";

struct AccountRuntime {
    provider: Arc<dyn MailProvider>,
    wake: Arc<Notify>,
    task: tauri::async_runtime::JoinHandle<()>,
}

pub struct AppState {
    pub db: SqlitePool,
    pub search: Arc<SearchIndex>,
    pub app: AppHandle,
    runtimes: RwLock<HashMap<i64, AccountRuntime>>,
}

pub async fn gmail_client_config(db: &SqlitePool) -> Result<GoogleClientConfig> {
    let client_id = store::setting(db, SETTING_GMAIL_CLIENT_ID)
        .await?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Invalid("Bitte zuerst die Google-OAuth-Client-ID in den Einstellungen hinterlegen".into()))?;
    let client_secret = keychain::get(keychain::GMAIL_CLIENT_SECRET_KEY)?
        .ok_or_else(|| AppError::Invalid("Bitte zuerst das Google-OAuth-Client-Secret in den Einstellungen hinterlegen".into()))?;
    Ok(GoogleClientConfig { client_id, client_secret })
}

pub fn server_configs(a: &Account) -> (ServerConfig, ServerConfig) {
    (
        ServerConfig {
            host: a.imap_host.clone(),
            port: a.imap_port as u16,
            security: Security::parse(&a.imap_security),
        },
        ServerConfig {
            host: a.smtp_host.clone(),
            port: a.smtp_port as u16,
            security: Security::parse(&a.smtp_security),
        },
    )
}

impl AppState {
    pub fn new(db: SqlitePool, search: Arc<SearchIndex>, app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            db,
            search,
            app,
            runtimes: RwLock::new(HashMap::new()),
        })
    }

    async fn build_provider(
        &self,
        account: &Account,
        seed: Option<(String, Duration)>,
    ) -> Result<Arc<dyn MailProvider>> {
        let kind = ProviderKind::parse(&account.kind);
        let (imap, smtp) = server_configs(account);
        let auth = match kind {
            ProviderKind::Imap => Auth::Password {
                username: account.username.clone(),
                password: keychain::get(&keychain::account_password_key(account.id))?
                    .ok_or_else(|| AppError::Auth(format!("Kein Passwort für {} im Schlüsselbund", account.email)))?,
            },
            ProviderKind::Gmail => {
                let tokens = Arc::new(GoogleTokenManager::new(account.id, gmail_client_config(&self.db).await?));
                if let Some((token, expires)) = seed {
                    tokens.seed(token, expires).await;
                }
                Auth::OAuth2 {
                    username: account.email.clone(),
                    tokens,
                }
            }
        };
        Ok(Arc::new(ImapSmtpProvider::new(kind, imap, smtp, auth)))
    }

    /// Creates the provider for an account and starts its background sync loop.
    pub async fn start_account(self: &Arc<Self>, account: &Account, seed: Option<(String, Duration)>) -> Result<()> {
        self.stop_account(account.id).await;
        let provider = self.build_provider(account, seed).await?;
        let wake = Arc::new(Notify::new());
        let task = sync::scheduler::spawn(self.clone(), account.id, provider.clone(), wake.clone());
        self.runtimes.write().await.insert(
            account.id,
            AccountRuntime { provider, wake, task },
        );
        Ok(())
    }

    pub async fn stop_account(&self, account_id: i64) {
        if let Some(rt) = self.runtimes.write().await.remove(&account_id) {
            rt.task.abort();
        }
    }

    pub async fn start_all(self: &Arc<Self>) -> Result<()> {
        for account in store::accounts::list(&self.db).await? {
            if let Err(e) = self.start_account(&account, None).await {
                tracing::error!("Konto {} konnte nicht gestartet werden: {e}", account.email);
                sync::scheduler::emit_status(&self.app, account.id, "error", Some(e.to_string()));
            }
        }
        Ok(())
    }

    pub async fn provider(&self, account_id: i64) -> Result<Arc<dyn MailProvider>> {
        self.runtimes
            .read()
            .await
            .get(&account_id)
            .map(|rt| rt.provider.clone())
            .ok_or_else(|| AppError::NotFound(format!("Konto {account_id} ist nicht aktiv")))
    }

    pub async fn wake(&self, account_id: Option<i64>) {
        let rts = self.runtimes.read().await;
        for (id, rt) in rts.iter() {
            if account_id.is_none_or(|a| a == *id) {
                rt.wake.notify_one();
            }
        }
    }
}
