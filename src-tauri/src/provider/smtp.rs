use std::time::Duration;

use lettre::address::Envelope;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{CertificateStore, Tls, TlsParameters};
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use super::{Auth, OutgoingEnvelope, Security, ServerConfig};
use crate::error::{AppError, Result};

async fn transport(cfg: &ServerConfig, auth: &Auth) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let tls_params = TlsParameters::builder(cfg.host.clone())
        .certificate_store(CertificateStore::Default)
        .build()?;
    // Implicit TLS or mandatory STARTTLS only; plaintext SMTP is never used.
    let tls = match cfg.security {
        Security::Tls => Tls::Wrapper(tls_params),
        Security::Starttls => Tls::Required(tls_params),
    };
    let (credentials, mechanisms) = match auth {
        Auth::Password { username, password } => (
            Credentials::new(username.clone(), password.clone()),
            vec![Mechanism::Plain, Mechanism::Login],
        ),
        Auth::OAuth2 { username, tokens } => (
            Credentials::new(username.clone(), tokens.access_token().await?),
            vec![Mechanism::Xoauth2],
        ),
    };
    Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host)
        .port(cfg.port)
        .tls(tls)
        .credentials(credentials)
        .authentication(mechanisms)
        .timeout(Some(Duration::from_secs(30)))
        .build())
}

pub async fn test(cfg: &ServerConfig, auth: &Auth) -> Result<()> {
    let ok = transport(cfg, auth).await?.test_connection().await?;
    if ok {
        Ok(())
    } else {
        Err(AppError::Invalid(format!("SMTP-Server {} antwortet nicht", cfg.host)))
    }
}

pub async fn send(cfg: &ServerConfig, auth: &Auth, raw: &[u8], env: &OutgoingEnvelope) -> Result<()> {
    let from: Address = env.from.parse()?;
    let to = env
        .to
        .iter()
        .map(|a| a.parse::<Address>())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let envelope = Envelope::new(Some(from), to)?;
    transport(cfg, auth).await?.send_raw(&envelope, raw).await?;
    Ok(())
}
