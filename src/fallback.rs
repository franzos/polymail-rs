use async_trait::async_trait;

use crate::email::Email;
use crate::error::SendError;
use crate::mailer::{BatchItemResult, Mailer, SendResult};

/// A mailer that tries multiple providers in order.
///
/// On a retryable failure (transport error, rate limit, service unavailable,
/// auth error), the next provider is tried. On a permanent failure (invalid
/// address, hard bounce, spam complaint), the error is returned immediately.
///
/// ```rust,ignore
/// use polymail::FallbackMailer;
/// use polymail::provider::lettermint::LettermintMailer;
/// use polymail::provider::postmark::PostmarkMailer;
///
/// let mailer = FallbackMailer::new(vec![
///     Box::new(LettermintMailer::new("lettermint-token")),
///     Box::new(PostmarkMailer::new("postmark-token")),
/// ]);
///
/// // Tries Lettermint first; if it's down, sends through Postmark.
/// let result = mailer.send(&email).await?;
/// ```
pub struct FallbackMailer {
    providers: Vec<Box<dyn Mailer>>,
}

impl FallbackMailer {
    pub fn new(providers: Vec<Box<dyn Mailer>>) -> Self {
        Self { providers }
    }
}

#[async_trait]
impl Mailer for FallbackMailer {
    async fn send(&self, email: &Email) -> Result<SendResult, SendError> {
        let mut errors = Vec::new();

        for provider in &self.providers {
            match provider.send(email).await {
                Ok(result) => return Ok(result),
                Err(e) if e.is_retryable() => {
                    errors.push(e);
                }
                Err(e) => return Err(e),
            }
        }

        Err(SendError::AllProvidersFailed(errors))
    }

    async fn batch_send(&self, emails: &[Email]) -> Result<Vec<BatchItemResult>, SendError> {
        if emails.is_empty() {
            return Err(SendError::BatchEmpty);
        }

        let mut errors = Vec::new();

        for provider in &self.providers {
            match provider.batch_send(emails).await {
                Ok(results) => return Ok(results),
                Err(e) if e.is_retryable() => {
                    errors.push(e);
                }
                Err(e) => return Err(e),
            }
        }

        Err(SendError::AllProvidersFailed(errors))
    }
}
