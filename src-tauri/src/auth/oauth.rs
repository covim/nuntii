//! Google OAuth2 (authorization code + PKCE) with a loopback redirect, plus an access-token
//! cache that refreshes transparently from the refresh token stored in the keychain.

use std::time::{Duration, Instant};

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::auth::keychain;
use crate::error::{AppError, Result};

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const GMAIL_SCOPES: [&str; 3] = ["https://mail.google.com/", "openid", "email"];
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const LOGO_SVG: &str = include_str!("../../app-icon.svg");

type GoogleClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Clone)]
pub struct GoogleClientConfig {
    pub client_id: String,
    pub client_secret: String,
}

fn http_client() -> Result<reqwest::Client> {
    // Redirects must not be followed for token requests (SSRF protection, see oauth2 docs).
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

fn google_client(cfg: &GoogleClientConfig) -> Result<GoogleClient> {
    let bad = |e: url::ParseError| AppError::OAuth(e.to_string());
    Ok(BasicClient::new(ClientId::new(cfg.client_id.clone()))
        .set_client_secret(ClientSecret::new(cfg.client_secret.clone()))
        .set_auth_uri(AuthUrl::new(GOOGLE_AUTH_URL.into()).map_err(bad)?)
        .set_token_uri(TokenUrl::new(GOOGLE_TOKEN_URL.into()).map_err(bad)?))
}

pub struct GoogleLogin {
    pub email: String,
    pub refresh_token: String,
    pub access_token: String,
    pub expires_in: Duration,
}

/// Runs the interactive login: opens the system browser and waits for the loopback redirect.
pub async fn google_login(
    cfg: &GoogleClientConfig,
    open_browser: impl FnOnce(&str) -> Result<()>,
) -> Result<GoogleLogin> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}");

    let client = google_client(cfg)?.set_redirect_uri(
        RedirectUrl::new(redirect).map_err(|e| AppError::OAuth(e.to_string()))?,
    );
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf) = client
        .authorize_url(CsrfToken::new_random)
        .add_scopes(GMAIL_SCOPES.iter().map(|s| Scope::new((*s).to_string())))
        .add_extra_param("access_type", "offline")
        .add_extra_param("prompt", "consent")
        .set_pkce_challenge(pkce_challenge)
        .url();

    open_browser(auth_url.as_str())?;

    let code = tokio::time::timeout(LOGIN_TIMEOUT, wait_for_code(&listener, csrf.secret()))
        .await
        .map_err(|_| AppError::OAuth("Zeitüberschreitung bei der Google-Anmeldung".into()))??;

    let http = http_client()?;
    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(&http)
        .await
        .map_err(|e| AppError::OAuth(format!("Token-Austausch fehlgeschlagen: {e}")))?;

    let refresh_token = token
        .refresh_token()
        .ok_or_else(|| AppError::OAuth("Google hat kein Refresh-Token geliefert".into()))?
        .secret()
        .clone();
    let access_token = token.access_token().secret().clone();
    let expires_in = token.expires_in().unwrap_or(Duration::from_secs(3000));

    #[derive(serde::Deserialize)]
    struct UserInfo {
        email: String,
    }
    let info: UserInfo = http
        .get(GOOGLE_USERINFO_URL)
        .bearer_auth(&access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(GoogleLogin {
        email: info.email,
        refresh_token,
        access_token,
        expires_in,
    })
}

/// Accepts loopback connections until one carries the OAuth redirect (browsers may also probe
/// e.g. /favicon.ico), then answers with a small confirmation page.
async fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await?;
        let request = String::from_utf8_lossy(&buf[..n]);
        let Some(path) = request
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
        else {
            continue;
        };
        let url = url::Url::parse(&format!("http://127.0.0.1{path}"))
            .map_err(|e| AppError::OAuth(e.to_string()))?;
        let param = |k: &str| {
            url.query_pairs()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
        };

        let (outcome, body) = match (param("code"), param("state"), param("error")) {
            (Some(code), Some(state), _) if state == expected_state => {
                (Some(Ok(code)), "Anmeldung erfolgreich. Sie können dieses Fenster schließen und zu nuntii zurückkehren.")
            }
            (Some(_), _, _) => (
                Some(Err(AppError::OAuth("Ungültiger state-Parameter".into()))),
                "Anmeldung abgelehnt (ungültiger state-Parameter).",
            ),
            (_, _, Some(err)) => (
                Some(Err(AppError::OAuth(format!("Google meldet: {err}")))),
                "Anmeldung abgebrochen.",
            ),
            _ => (None, "Not found"),
        };

        let page = format!(
            "<!doctype html><meta charset=utf-8><title>nuntii</title>\
             <body style=\"font-family:system-ui,sans-serif;padding:3em;display:flex;gap:1em;align-items:center\">\
             <div style=\"width:48px;height:48px;flex:none\">{LOGO_SVG}</div><p>{body}</p></body>"
        );
        let status = if outcome.is_some() { "200 OK" } else { "404 Not Found" };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;

        if let Some(result) = outcome {
            return result;
        }
    }
}

/// Hands out valid access tokens for one Gmail account, refreshing them when they expire.
pub struct GoogleTokenManager {
    account_id: i64,
    cfg: GoogleClientConfig,
    cached: Mutex<Option<(String, Instant)>>,
}

impl GoogleTokenManager {
    pub fn new(account_id: i64, cfg: GoogleClientConfig) -> Self {
        Self {
            account_id,
            cfg,
            cached: Mutex::new(None),
        }
    }

    pub async fn seed(&self, access_token: String, expires_in: Duration) {
        *self.cached.lock().await = Some((access_token, Instant::now() + expires_in));
    }

    /// Drops the cached token so the next call refreshes (used after an AUTH failure).
    pub async fn invalidate(&self) {
        *self.cached.lock().await = None;
    }

    pub async fn access_token(&self) -> Result<String> {
        let mut cached = self.cached.lock().await;
        if let Some((token, expiry)) = cached.as_ref() {
            if Instant::now() + Duration::from_secs(60) < *expiry {
                return Ok(token.clone());
            }
        }
        let refresh = keychain::get(&keychain::account_refresh_token_key(self.account_id))?
            .ok_or_else(|| {
                AppError::Auth("Kein Refresh-Token gespeichert – bitte Gmail neu verbinden".into())
            })?;
        let token = google_client(&self.cfg)?
            .exchange_refresh_token(&RefreshToken::new(refresh))
            .request_async(&http_client()?)
            .await
            .map_err(|e| AppError::Auth(format!("Token-Erneuerung fehlgeschlagen: {e}")))?;
        // Google may rotate the refresh token.
        if let Some(rt) = token.refresh_token() {
            keychain::set(
                &keychain::account_refresh_token_key(self.account_id),
                rt.secret(),
            )?;
        }
        let access = token.access_token().secret().clone();
        let expiry = Instant::now() + token.expires_in().unwrap_or(Duration::from_secs(3000));
        *cached = Some((access.clone(), expiry));
        Ok(access)
    }
}
