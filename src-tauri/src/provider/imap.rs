//! IMAP/SMTP provider used for generic IMAP servers (Synology MailPlus) and Gmail (XOAUTH2).

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_imap::types::{Fetch, Flag, NameAttribute};
use async_imap::{Authenticator, Client, Session};
use async_trait::async_trait;
use futures::TryStreamExt;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use super::{
    smtp, uid_set, Auth, FlagKind, FlagUpdate, Flags, FolderRole, FolderStatus, MailProvider,
    OutgoingEnvelope, ProviderKind, RemoteFolder, RemoteHeader, Security, ServerConfig,
};
use crate::error::{AppError, Result};

type ImapSession = Session<TlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const IDLE_CHECK_AFTER: Duration = Duration::from_secs(120);
const MAX_POOLED: usize = 2;

struct Conn {
    session: ImapSession,
    condstore: bool,
    has_move: bool,
    uidplus: bool,
    last_used: Instant,
}

pub struct ImapSmtpProvider {
    kind: ProviderKind,
    imap: ServerConfig,
    smtp: ServerConfig,
    auth: Auth,
    pool: Mutex<Vec<Conn>>,
}

fn tls_connector() -> Result<TlsConnector> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    if let Some(cfg) = CONFIG.get() {
        return Ok(TlsConnector::from(cfg.clone()));
    }
    use rustls_platform_verifier::ConfigVerifierExt;
    // Verifies against the OS trust store, so self-hosted servers (e.g. a Synology NAS with an
    // internal CA installed on the machine) work without disabling certificate checks.
    let cfg = Arc::new(
        rustls::ClientConfig::with_platform_verifier()
            .map_err(|e| AppError::Tls(e.to_string()))?,
    );
    Ok(TlsConnector::from(CONFIG.get_or_init(|| cfg).clone()))
}

struct XOAuth2 {
    user: String,
    token: String,
    sent: bool,
}

impl Authenticator for XOAuth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        // The second challenge (if any) carries a JSON error; answering empty makes the server
        // finish with a tagged NO.
        if self.sent {
            return String::new();
        }
        self.sent = true;
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

impl ImapSmtpProvider {
    pub fn new(kind: ProviderKind, imap: ServerConfig, smtp: ServerConfig, auth: Auth) -> Self {
        Self {
            kind,
            imap,
            smtp,
            auth,
            pool: Mutex::new(Vec::new()),
        }
    }

    async fn open_tls(&self) -> Result<TlsStream<TcpStream>> {
        let addr = (self.imap.host.as_str(), self.imap.port);
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| AppError::Invalid(format!("Zeitüberschreitung beim Verbinden mit {}", self.imap.host)))??;
        let name = ServerName::try_from(self.imap.host.clone())
            .map_err(|e| AppError::Tls(e.to_string()))?;
        let connector = tls_connector()?;

        match self.imap.security {
            Security::Tls => Ok(connector.connect(name, tcp).await?),
            Security::Starttls => {
                let mut plain = Client::new(tcp);
                plain
                    .read_response()
                    .await?
                    .ok_or_else(|| AppError::Invalid("Keine Begrüßung vom IMAP-Server".into()))?;
                // No plaintext fallback: if STARTTLS is refused, the connection fails.
                plain
                    .run_command_and_check_ok("STARTTLS", None)
                    .await
                    .map_err(|e| AppError::Tls(format!("Server unterstützt kein STARTTLS: {e}")))?;
                Ok(connector.connect(name, plain.into_inner()).await?)
            }
        }
    }

    async fn connect(&self) -> Result<Conn> {
        let session = match self.login_once().await {
            Err(AppError::Auth(_)) if matches!(self.auth, Auth::OAuth2 { .. }) => {
                if let Auth::OAuth2 { tokens, .. } = &self.auth {
                    tokens.invalidate().await;
                }
                self.login_once().await?
            }
            other => other?,
        };
        let mut session = session;
        let caps = session.capabilities().await?;
        Ok(Conn {
            condstore: caps.has_str("CONDSTORE"),
            has_move: caps.has_str("MOVE"),
            uidplus: caps.has_str("UIDPLUS"),
            session,
            last_used: Instant::now(),
        })
    }

    async fn login_once(&self) -> Result<ImapSession> {
        let tls = self.open_tls().await?;
        let mut client = Client::new(tls);
        if self.imap.security == Security::Tls {
            client
                .read_response()
                .await?
                .ok_or_else(|| AppError::Invalid("Keine Begrüßung vom IMAP-Server".into()))?;
        }
        match &self.auth {
            Auth::Password { username, password } => client
                .login(username, password)
                .await
                .map_err(|(e, _)| AppError::Auth(e.to_string())),
            Auth::OAuth2 { username, tokens } => {
                let token = tokens.access_token().await?;
                let auth = XOAuth2 {
                    user: username.clone(),
                    token,
                    sent: false,
                };
                client
                    .authenticate("XOAUTH2", auth)
                    .await
                    .map_err(|(e, _)| AppError::Auth(e.to_string()))
            }
        }
    }

    async fn acquire(&self) -> Result<Conn> {
        loop {
            let pooled = self.pool.lock().await.pop();
            let Some(mut conn) = pooled else {
                return self.connect().await;
            };
            if conn.last_used.elapsed() < IDLE_CHECK_AFTER {
                return Ok(conn);
            }
            // Servers drop idle connections; probe before reuse.
            if conn.session.noop().await.is_ok() {
                return Ok(conn);
            }
        }
    }

    async fn release(&self, mut conn: Conn, healthy: bool) {
        if !healthy {
            return;
        }
        conn.last_used = Instant::now();
        let mut pool = self.pool.lock().await;
        if pool.len() < MAX_POOLED {
            pool.push(conn);
        }
    }

    async fn select(conn: &mut Conn, folder: &str) -> Result<async_imap::types::Mailbox> {
        Ok(if conn.condstore {
            conn.session.select_condstore(folder).await?
        } else {
            conn.session.select(folder).await?
        })
    }
}

/// Runs `$body` with a pooled connection bound to `$conn`, returning it to the pool on success.
macro_rules! with_conn {
    ($self:ident, |$conn:ident| $body:expr) => {{
        let mut owned = $self.acquire().await?;
        let result: Result<_> = async {
            let $conn = &mut owned;
            $body
        }
        .await;
        $self.release(owned, result.is_ok()).await;
        result
    }};
}

fn flags_of(fetch: &Fetch) -> Flags {
    let mut f = Flags::default();
    for flag in fetch.flags() {
        match flag {
            Flag::Seen => f.seen = true,
            Flag::Flagged => f.flagged = true,
            Flag::Answered => f.answered = true,
            Flag::Draft => f.draft = true,
            Flag::Deleted => f.deleted = true,
            _ => {}
        }
    }
    f
}

fn imap_date(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .unwrap_or_default()
        .format("%d-%b-%Y")
        .to_string()
}

/// Maps a folder to its role from SPECIAL-USE attributes, falling back to common names.
fn folder_role(name: &str, delimiter: Option<&str>, attrs: &[NameAttribute<'_>]) -> Option<FolderRole> {
    if name.eq_ignore_ascii_case("INBOX") {
        return Some(FolderRole::Inbox);
    }
    for a in attrs {
        let role = match a {
            NameAttribute::Sent => FolderRole::Sent,
            NameAttribute::Drafts => FolderRole::Drafts,
            NameAttribute::Trash => FolderRole::Trash,
            NameAttribute::Archive => FolderRole::Archive,
            NameAttribute::Junk => FolderRole::Junk,
            NameAttribute::All | NameAttribute::Flagged => FolderRole::Virtual,
            NameAttribute::Extension(e) if e.eq_ignore_ascii_case("\\Important") => FolderRole::Virtual,
            _ => continue,
        };
        return Some(role);
    }
    let decoded = decode_modified_utf7(name);
    let leaf = match delimiter {
        Some(d) if !d.is_empty() => decoded.rsplit(d).next().unwrap_or(&decoded).to_string(),
        _ => decoded,
    }
    .to_lowercase();
    match leaf.as_str() {
        "sent" | "sent messages" | "sent items" | "sent mail" | "gesendet" | "gesendete objekte"
        | "gesendete elemente" => Some(FolderRole::Sent),
        "drafts" | "entwürfe" => Some(FolderRole::Drafts),
        "trash" | "deleted messages" | "deleted items" | "papierkorb" | "gelöschte elemente"
        | "gelöschte objekte" => Some(FolderRole::Trash),
        "junk" | "spam" | "junk e-mail" => Some(FolderRole::Junk),
        "archive" | "archiv" => Some(FolderRole::Archive),
        _ => None,
    }
}

/// Decodes IMAP "modified UTF-7" mailbox names (RFC 3501 §5.1.3) for display.
pub fn decode_modified_utf7(input: &str) -> String {
    use base64::Engine;
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let chunk = &after[..end];
        if chunk.is_empty() {
            out.push('&');
        } else {
            let b64 = chunk.replace(',', "/");
            match base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64.trim_end_matches('=')) {
                Ok(bytes) => {
                    let units: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    out.push_str(&String::from_utf16_lossy(&units));
                }
                Err(_) => out.push_str(&rest[start..start + 1 + end + 1]),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

async fn drain<S, T>(stream: S) -> Result<Vec<T>>
where
    S: futures::Stream<Item = async_imap::error::Result<T>>,
{
    Ok(stream.try_collect::<Vec<T>>().await?)
}

#[async_trait]
impl MailProvider for ImapSmtpProvider {
    async fn test(&self) -> Result<()> {
        let conn = self.connect().await?;
        self.release(conn, true).await;
        smtp::test(&self.smtp, &self.auth).await
    }

    async fn list_folders(&self) -> Result<Vec<RemoteFolder>> {
        with_conn!(self, |c| {
            let names = drain(c.session.list(Some(""), Some("*")).await?).await?;
            Ok(names
                .iter()
                .map(|n| {
                    let attrs = n.attributes();
                    RemoteFolder {
                        name: n.name().to_string(),
                        delimiter: n.delimiter().map(str::to_string),
                        role: folder_role(n.name(), n.delimiter(), attrs),
                        selectable: !attrs.iter().any(|a| matches!(a, NameAttribute::NoSelect)),
                    }
                })
                .collect())
        })
    }

    async fn folder_status(&self, folder: &str) -> Result<FolderStatus> {
        with_conn!(self, |c| {
            let mb = Self::select(c, folder).await?;
            Ok(FolderStatus {
                uidvalidity: mb.uid_validity.unwrap_or(0),
                uidnext: mb.uid_next.unwrap_or(0),
                highestmodseq: mb.highest_modseq,
                exists: mb.exists,
            })
        })
    }

    async fn all_uids(&self, folder: &str) -> Result<Vec<u32>> {
        with_conn!(self, |c| {
            Self::select(c, folder).await?;
            Ok(c.session.uid_search("ALL").await?.into_iter().collect())
        })
    }

    async fn uids_since(&self, folder: &str, since: i64) -> Result<Vec<u32>> {
        with_conn!(self, |c| {
            Self::select(c, folder).await?;
            let query = format!("SINCE {}", imap_date(since));
            Ok(c.session.uid_search(query).await?.into_iter().collect())
        })
    }

    async fn uids_from(&self, folder: &str, min_uid: u32) -> Result<Vec<u32>> {
        with_conn!(self, |c| {
            Self::select(c, folder).await?;
            let query = format!("UID {}:*", min_uid.max(1));
            // "n:*" always matches the highest UID, even if it is below n.
            Ok(c.session.uid_search(query).await?.into_iter().filter(|u| *u >= min_uid).collect())
        })
    }

    async fn fetch_headers(&self, folder: &str, uids: &[u32]) -> Result<Vec<RemoteHeader>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        with_conn!(self, |c| {
            Self::select(c, folder).await?;
            let fetches = drain(
                c.session
                    .uid_fetch(uid_set(uids), "(UID FLAGS RFC822.SIZE INTERNALDATE BODY.PEEK[HEADER])")
                    .await?,
            )
            .await?;
            Ok(fetches
                .iter()
                .filter_map(|f| {
                    Some(RemoteHeader {
                        uid: f.uid?,
                        flags: flags_of(f),
                        size: f.size.unwrap_or(0),
                        internal_date: f.internal_date().map(|d| d.timestamp()),
                        header: f.header()?.to_vec(),
                    })
                })
                .collect())
        })
    }

    async fn fetch_flags(
        &self,
        folder: &str,
        min_uid: u32,
        modseq: Option<u64>,
    ) -> Result<Vec<FlagUpdate>> {
        with_conn!(self, |c| {
            Self::select(c, folder).await?;
            let query = match modseq {
                Some(m) if c.condstore => format!("(UID FLAGS) (CHANGEDSINCE {m})"),
                _ => "(UID FLAGS)".to_string(),
            };
            let fetches = drain(c.session.uid_fetch(format!("{}:*", min_uid.max(1)), query).await?).await?;
            Ok(fetches
                .iter()
                .filter_map(|f| {
                    let uid = f.uid?;
                    (uid >= min_uid).then(|| FlagUpdate { uid, flags: flags_of(f) })
                })
                .collect())
        })
    }

    async fn fetch_raw(&self, folder: &str, uids: &[u32]) -> Result<Vec<(u32, Vec<u8>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        with_conn!(self, |c| {
            Self::select(c, folder).await?;
            let fetches = drain(c.session.uid_fetch(uid_set(uids), "(UID BODY.PEEK[])").await?).await?;
            Ok(fetches
                .iter()
                .filter_map(|f| Some((f.uid?, f.body()?.to_vec())))
                .collect())
        })
    }

    async fn set_flag(&self, folder: &str, uids: &[u32], flag: FlagKind, on: bool) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        with_conn!(self, |c| {
            Self::select(c, folder).await?;
            let op = if on { "+FLAGS.SILENT" } else { "-FLAGS.SILENT" };
            drain(c.session.uid_store(uid_set(uids), format!("{op} ({})", flag.imap())).await?).await?;
            Ok(())
        })
    }

    async fn move_messages(&self, from: &str, uids: &[u32], to: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        with_conn!(self, |c| {
            Self::select(c, from).await?;
            let set = uid_set(uids);
            if c.has_move {
                c.session.uid_mv(&set, to).await?;
            } else {
                c.session.uid_copy(&set, to).await?;
                drain(c.session.uid_store(&set, "+FLAGS.SILENT (\\Deleted)").await?).await?;
                if c.uidplus {
                    drain(c.session.uid_expunge(&set).await?).await?;
                } else {
                    drain(c.session.expunge().await?).await?;
                }
            }
            Ok(())
        })
    }

    async fn delete_permanently(&self, folder: &str, uids: &[u32]) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        with_conn!(self, |c| {
            Self::select(c, folder).await?;
            let set = uid_set(uids);
            drain(c.session.uid_store(&set, "+FLAGS.SILENT (\\Deleted)").await?).await?;
            if c.uidplus {
                drain(c.session.uid_expunge(&set).await?).await?;
            } else {
                drain(c.session.expunge().await?).await?;
            }
            Ok(())
        })
    }

    async fn append(&self, folder: &str, raw: &[u8], seen: bool) -> Result<()> {
        with_conn!(self, |c| {
            let flags = seen.then_some("(\\Seen)");
            c.session.append(folder, flags, None, raw).await?;
            Ok(())
        })
    }

    async fn send(&self, raw: &[u8], envelope: &OutgoingEnvelope) -> Result<()> {
        smtp::send(&self.smtp, &self.auth, raw, envelope).await
    }

    fn stores_sent_automatically(&self) -> bool {
        self.kind == ProviderKind::Gmail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_modified_utf7() {
        assert_eq!(decode_modified_utf7("INBOX"), "INBOX");
        assert_eq!(decode_modified_utf7("Entw&APw-rfe"), "Entwürfe");
        assert_eq!(decode_modified_utf7("Gel&APY-schte Elemente"), "Gelöschte Elemente");
        assert_eq!(decode_modified_utf7("A &- B"), "A & B");
    }

    #[test]
    fn detects_roles_by_name() {
        assert_eq!(folder_role("INBOX", Some("/"), &[]), Some(FolderRole::Inbox));
        assert_eq!(folder_role("INBOX/Gesendet", Some("/"), &[]), Some(FolderRole::Sent));
        assert_eq!(folder_role("Entw&APw-rfe", Some("."), &[]), Some(FolderRole::Drafts));
        assert_eq!(folder_role("[Gmail]/All Mail", Some("/"), &[NameAttribute::All]), Some(FolderRole::Virtual));
        assert_eq!(folder_role("Projekte", Some("/"), &[]), None);
    }
}

