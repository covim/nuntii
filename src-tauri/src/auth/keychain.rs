//! Secrets (passwords, refresh tokens, OAuth client secrets) live exclusively in the OS keychain:
//! Windows Credential Manager, macOS Keychain or the Linux Secret Service.

use crate::error::Result;

const SERVICE: &str = "nuntii";

fn entry(key: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, key)?)
}

pub fn set(key: &str, secret: &str) -> Result<()> {
    entry(key)?.set_password(secret)?;
    Ok(())
}

pub fn get(key: &str) -> Result<Option<String>> {
    match entry(key)?.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete(key: &str) -> Result<()> {
    match entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn account_password_key(account_id: i64) -> String {
    format!("account:{account_id}:password")
}

pub fn account_refresh_token_key(account_id: i64) -> String {
    format!("account:{account_id}:refresh_token")
}

pub const GMAIL_CLIENT_SECRET_KEY: &str = "gmail:client_secret";
