//! Deserializable provider configuration (`config` feature).
//!
//! Lets an application load its mail settings from TOML, env, or any other
//! `serde` source and turn them into a ready [`Mailer`] without hand-writing
//! the per-provider match. The application owns the loading; this is only the
//! schema plus the glue.

use serde::Deserialize;

use crate::error::SendError;
use crate::mailer::Mailer;

/// A single mail provider, as loaded from configuration.
///
/// Internally tagged on `provider`, so a TOML table reads:
///
/// ```toml
/// provider = "lettermint"
/// token = "your-api-token"
/// ```
///
/// Only variants whose provider feature is enabled at compile time exist. A
/// config naming a provider that wasn't compiled in fails to deserialize with
/// an "unknown variant" error that lists the ones that are available.
///
/// Build it with [`ProviderConfig::build`]; for a fallback chain, pass several
/// to [`FallbackMailer::from_configs`](crate::FallbackMailer::from_configs).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ProviderConfig {
    #[cfg(feature = "lettermint")]
    Lettermint { token: String },

    #[cfg(feature = "postmark")]
    Postmark { token: String },

    #[cfg(feature = "sendgrid")]
    Sendgrid { api_key: String },

    #[cfg(feature = "smtp")]
    Smtp {
        host: String,
        /// Defaults to the provider default for the chosen TLS mode when omitted
        /// (465 for `implicit`, 587 for `start_tls`).
        #[serde(default)]
        port: Option<u16>,
        #[serde(default)]
        tls: crate::provider::smtp::SmtpTls,
        #[serde(default)]
        user: Option<String>,
        #[serde(default)]
        pass: Option<String>,
    },
}

impl ProviderConfig {
    /// Build the configured provider into a ready-to-use mailer.
    pub fn build(self) -> Result<Box<dyn Mailer>, SendError> {
        match self {
            #[cfg(feature = "lettermint")]
            ProviderConfig::Lettermint { token } => Ok(Box::new(
                crate::provider::lettermint::LettermintMailer::new(token),
            )),
            #[cfg(feature = "postmark")]
            ProviderConfig::Postmark { token } => Ok(Box::new(
                crate::provider::postmark::PostmarkMailer::new(token),
            )),
            #[cfg(feature = "sendgrid")]
            ProviderConfig::Sendgrid { api_key } => Ok(Box::new(
                crate::provider::sendgrid::SendgridMailer::new(api_key),
            )),
            #[cfg(feature = "smtp")]
            ProviderConfig::Smtp {
                host,
                port,
                tls,
                user,
                pass,
            } => {
                let mut b = crate::provider::smtp::SmtpMailer::builder(host).tls(tls);
                if let Some(p) = port {
                    b = b.port(p);
                }
                if let (Some(u), Some(pw)) = (user, pass) {
                    b = b.credentials(u, pw);
                }
                Ok(Box::new(b.build()?))
            }
        }
    }
}
