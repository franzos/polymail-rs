/// Errors that can occur when sending an email.
///
/// Provider-specific error codes are mapped to these shared variants so
/// callers can handle common failure modes without matching on providers.
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    /// Transport or client-level failure (network, TLS, timeout).
    #[error("provider error: {0}")]
    Provider(String),

    /// Invalid API key or insufficient permissions.
    #[error("authentication error: {0}")]
    Authentication(String),

    /// Invalid email address or failed validation.
    #[error("invalid address: {0}")]
    InvalidAddress(String),

    /// Recipient has been marked inactive (hard suppression).
    #[error("inactive recipient: {0}")]
    InactiveRecipient(String),

    /// Recipient has previously filed a spam complaint.
    #[error("spam complaint: {0}")]
    SpamComplaint(String),

    /// Recipient address hard-bounced.
    #[error("hard bounce: {0}")]
    HardBounce(String),

    /// Rate limit exceeded; retry later.
    #[error("rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    /// Remote service returned 5xx or is otherwise unavailable.
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Provider returned a non-success response that doesn't map to a specific variant.
    #[error("api error {status}: {message}")]
    Api { status: u16, message: String },

    /// Failed to serialize the request payload.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Batch request exceeds the provider's maximum.
    #[error("batch too large: {count} emails exceeds provider limit of {limit}")]
    BatchTooLarge { count: usize, limit: usize },

    /// Batch request was empty.
    #[error("batch is empty")]
    BatchEmpty,

    /// All providers in a fallback chain failed.
    #[error("all providers failed: {0:?}")]
    AllProvidersFailed(Vec<SendError>),
}

impl SendError {
    /// Whether this error is transient and a different provider might succeed.
    ///
    /// Returns `true` for transport failures, rate limits, and service outages.
    /// Returns `false` for permanent failures like invalid addresses or bounces
    /// — those will fail on any provider.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SendError::Provider(_)
                | SendError::RateLimitExceeded(_)
                | SendError::ServiceUnavailable(_)
                | SendError::Authentication(_)
        )
    }
}
