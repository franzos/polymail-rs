use async_trait::async_trait;

use crate::email::Email;
use crate::error::SendError;

/// The result of a successful single send.
#[derive(Debug, Clone)]
pub struct SendResult {
    /// Provider-assigned message ID, if available.
    pub message_id: Option<String>,
}

/// The result of a single email within a batch.
#[derive(Debug)]
pub enum BatchItemResult {
    Success(SendResult),
    Failed(SendError),
}

/// Trait implemented by each email provider.
#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, email: &Email) -> Result<SendResult, SendError>;

    /// Send multiple emails in a single batch request.
    ///
    /// Providers with native batch support (Lettermint, Postmark) send all
    /// emails in one API call. Others fall back to sequential sends.
    ///
    /// Returns one result per input email, in the same order.
    async fn batch_send(&self, emails: &[Email]) -> Result<Vec<BatchItemResult>, SendError> {
        if emails.is_empty() {
            return Err(SendError::BatchEmpty);
        }

        let mut results = Vec::with_capacity(emails.len());
        for email in emails {
            results.push(match self.send(email).await {
                Ok(r) => BatchItemResult::Success(r),
                Err(e) => BatchItemResult::Failed(e),
            });
        }
        Ok(results)
    }
}
