use async_trait::async_trait;
use lettre::transport::smtp::Error as SmtpError;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use crate::email::Email;
use crate::error::SendError;
use crate::mailer::{Mailer, SendResult};
use crate::mime::to_message;

/// Transport security for the SMTP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "config", derive(serde::Deserialize, Default))]
#[cfg_attr(feature = "config", serde(rename_all = "snake_case"))]
pub enum SmtpTls {
    /// Plaintext, no encryption (e.g. mailcrab on 1025).
    None,
    /// Connect plaintext then upgrade via STARTTLS (typically port 587).
    StartTls,
    /// TLS from the first byte (typically port 465).
    #[cfg_attr(feature = "config", default)]
    Implicit,
}

fn map_smtp_error(e: SmtpError) -> SendError {
    if let Some(code) = e.status() {
        let code = u16::from(code);
        match code {
            535 | 530 | 534 | 538 | 454 => SendError::Authentication(e.to_string()),
            550 | 551 | 553 => SendError::HardBounce(e.to_string()),
            _ if (400..500).contains(&code) => SendError::ServiceUnavailable(e.to_string()),
            _ => SendError::Api {
                status: code,
                message: e.to_string(),
            },
        }
    } else {
        SendError::Provider(e.to_string())
    }
}

pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl SmtpMailer {
    pub fn builder(host: impl Into<String>) -> SmtpBuilder {
        SmtpBuilder {
            host: host.into(),
            port: None,
            tls: SmtpTls::Implicit,
            credentials: None,
        }
    }

    /// Plaintext, no-auth mailer for local testing (e.g. mailcrab, MailHog).
    pub fn plaintext(host: impl Into<String>, port: u16) -> Self {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host.into())
            .port(port)
            .build();
        Self { transport }
    }

    /// Escape hatch for a fully-configured lettre transport (custom TLS roots,
    /// pool tuning, alternate auth mechanisms).
    pub fn with_transport(transport: AsyncSmtpTransport<Tokio1Executor>) -> Self {
        Self { transport }
    }
}

pub struct SmtpBuilder {
    host: String,
    port: Option<u16>,
    tls: SmtpTls,
    credentials: Option<(String, String)>,
}

impl SmtpBuilder {
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn tls(mut self, tls: SmtpTls) -> Self {
        self.tls = tls;
        self
    }

    pub fn credentials(mut self, user: impl Into<String>, pass: impl Into<String>) -> Self {
        self.credentials = Some((user.into(), pass.into()));
        self
    }

    pub fn build(self) -> Result<SmtpMailer, SendError> {
        if self.credentials.is_some() && self.tls == SmtpTls::None {
            return Err(SendError::Authentication(
                "refusing to send credentials over a plaintext (SmtpTls::None) connection".into(),
            ));
        }
        let mut b = match self.tls {
            SmtpTls::Implicit => {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&self.host).map_err(map_smtp_error)?
            }
            SmtpTls::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.host)
                .map_err(map_smtp_error)?,
            SmtpTls::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.host),
        };
        if let Some(p) = self.port {
            b = b.port(p);
        }
        if let Some((u, pw)) = self.credentials {
            b = b.credentials(Credentials::new(u, pw));
        }
        Ok(SmtpMailer {
            transport: b.build(),
        })
    }
}

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, email: &Email) -> Result<SendResult, SendError> {
        let msg = to_message(email)?;
        self.transport.send(msg).await.map_err(map_smtp_error)?;
        Ok(SendResult { message_id: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::{Address, Body};

    fn base_email(body: Body) -> Email {
        Email::builder(Address::new("sender@example.com"), "Subject", body)
            .to("recipient@example.com")
            .build()
            .unwrap()
    }

    #[test]
    fn credentials_over_plaintext_are_rejected() {
        let result = SmtpMailer::builder("localhost")
            .tls(SmtpTls::None)
            .credentials("user", "pass")
            .build();
        assert!(matches!(result, Err(SendError::Authentication(_))));
    }

    #[tokio::test]
    #[ignore = "requires a running SMTP sink on localhost:1025 (mailcrab in CI)"]
    async fn plaintext_send_returns_no_message_id() {
        let mailer = SmtpMailer::plaintext("localhost", 1025);
        let email = base_email(Body::Text("hello sink".into()));
        let result = mailer.send(&email).await.unwrap();
        assert!(result.message_id.is_none());
    }
}
