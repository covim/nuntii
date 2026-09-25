use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Datenbankfehler: {0}")]
    Db(#[from] sqlx::Error),
    #[error("Migrationsfehler: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("IMAP-Fehler: {0}")]
    Imap(#[from] async_imap::error::Error),
    #[error("SMTP-Fehler: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
    #[error("Ungültige Nachricht: {0}")]
    MailBuild(#[from] lettre::error::Error),
    #[error("Ungültige Adresse: {0}")]
    Address(#[from] lettre::address::AddressError),
    #[error("Schlüsselbund-Fehler: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("Suchindex-Fehler: {0}")]
    Search(#[from] tantivy::TantivyError),
    #[error("E/A-Fehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("TLS-Fehler: {0}")]
    Tls(String),
    #[error("Anmeldung fehlgeschlagen: {0}")]
    Auth(String),
    #[error("OAuth-Fehler: {0}")]
    OAuth(String),
    #[error("Nicht gefunden: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("Keine Verbindung zum Server – Nachricht ist noch nicht offline verfügbar")]
    Offline,
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T, E = AppError> = std::result::Result<T, E>;
