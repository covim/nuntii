//! Protocol-agnostic mail provider interface. The sync engine and commands only talk to
//! `MailProvider`; IMAP/SMTP (Synology, Gmail) implements it today, Microsoft Graph later.

pub mod imap;
pub mod smtp;

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::oauth::GoogleTokenManager;
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    /// Implicit TLS (IMAPS 993 / SMTPS 465).
    Tls,
    /// STARTTLS, mandatory – the connection is aborted if the server does not offer it.
    Starttls,
}

impl Security {
    pub fn parse(s: &str) -> Self {
        if s == "starttls" {
            Security::Starttls
        } else {
            Security::Tls
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Imap,
    Gmail,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Self {
        if s == "gmail" {
            ProviderKind::Gmail
        } else {
            ProviderKind::Imap
        }
    }
}

#[derive(Clone)]
pub enum Auth {
    Password { username: String, password: String },
    OAuth2 { username: String, tokens: Arc<GoogleTokenManager> },
}

#[derive(Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Archive,
    Junk,
    /// Virtual "all mail"/"starred"/"important" views that only duplicate other folders.
    Virtual,
}

impl FolderRole {
    pub fn as_str(self) -> &'static str {
        match self {
            FolderRole::Inbox => "inbox",
            FolderRole::Sent => "sent",
            FolderRole::Drafts => "drafts",
            FolderRole::Trash => "trash",
            FolderRole::Archive => "archive",
            FolderRole::Junk => "junk",
            FolderRole::Virtual => "virtual",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteFolder {
    pub name: String,
    pub delimiter: Option<String>,
    pub role: Option<FolderRole>,
    pub selectable: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FolderStatus {
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub highestmodseq: Option<u64>,
    pub exists: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
    pub deleted: bool,
}

#[derive(Debug, Clone)]
pub struct RemoteHeader {
    pub uid: u32,
    pub flags: Flags,
    pub size: u32,
    pub internal_date: Option<i64>,
    /// Raw RFC 5322 header block, parsed by `mime::parse`.
    pub header: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FlagUpdate {
    pub uid: u32,
    pub flags: Flags,
}

#[derive(Debug, Clone, Copy)]
pub enum FlagKind {
    Seen,
    Flagged,
    Answered,
}

impl FlagKind {
    pub fn imap(self) -> &'static str {
        match self {
            FlagKind::Seen => "\\Seen",
            FlagKind::Flagged => "\\Flagged",
            FlagKind::Answered => "\\Answered",
        }
    }
}

pub struct OutgoingEnvelope {
    pub from: String,
    pub to: Vec<String>,
}

#[async_trait]
pub trait MailProvider: Send + Sync {
    /// Verifies that both the incoming and outgoing server accept the credentials.
    async fn test(&self) -> Result<()>;
    async fn list_folders(&self) -> Result<Vec<RemoteFolder>>;
    async fn folder_status(&self, folder: &str) -> Result<FolderStatus>;
    /// UIDs of all messages in the folder (used to detect remote deletions).
    async fn all_uids(&self, folder: &str) -> Result<Vec<u32>>;
    /// UIDs of messages received on or after the given unix timestamp.
    async fn uids_since(&self, folder: &str, since: i64) -> Result<Vec<u32>>;
    /// UIDs >= `min_uid`.
    async fn uids_from(&self, folder: &str, min_uid: u32) -> Result<Vec<u32>>;
    async fn fetch_headers(&self, folder: &str, uids: &[u32]) -> Result<Vec<RemoteHeader>>;
    /// Flags for all messages with UID >= `min_uid`; with CONDSTORE only those changed since `modseq`.
    async fn fetch_flags(
        &self,
        folder: &str,
        min_uid: u32,
        modseq: Option<u64>,
    ) -> Result<Vec<FlagUpdate>>;
    async fn fetch_raw(&self, folder: &str, uids: &[u32]) -> Result<Vec<(u32, Vec<u8>)>>;
    async fn set_flag(&self, folder: &str, uids: &[u32], flag: FlagKind, on: bool) -> Result<()>;
    async fn move_messages(&self, from: &str, uids: &[u32], to: &str) -> Result<()>;
    async fn delete_permanently(&self, folder: &str, uids: &[u32]) -> Result<()>;
    async fn append(&self, folder: &str, raw: &[u8], seen: bool) -> Result<()>;
    async fn send(&self, raw: &[u8], envelope: &OutgoingEnvelope) -> Result<()>;
    /// Whether the server files sent mail itself (Gmail), so no APPEND to "Sent" is needed.
    fn stores_sent_automatically(&self) -> bool {
        false
    }
}

/// Formats a UID list as a compact IMAP sequence set ("1:5,8,10:12").
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted: Vec<u32> = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut out = String::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        if !out.is_empty() {
            out.push(',');
        }
        if start == end {
            out.push_str(&start.to_string());
        } else {
            out.push_str(&format!("{start}:{end}"));
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::uid_set;

    #[test]
    fn compacts_uid_ranges() {
        assert_eq!(uid_set(&[5, 1, 2, 3, 8, 10, 11, 12, 3]), "1:3,5,8,10:12");
        assert_eq!(uid_set(&[7]), "7");
        assert_eq!(uid_set(&[]), "");
    }
}
