use async_trait::async_trait;
use lettermint::Query;
use lettermint::QueryError;
use lettermint::api::email::{
    Attachment as LmAttachment, BatchSendRequest, EmailStatus, SendEmailRequest, SendEmailResponse,
};
use lettermint::reqwest::{LettermintClient, LettermintClientError};

use crate::email::{Body, Email};
use crate::error::SendError;
use crate::mailer::{BatchItemResult, Mailer, SendResult};

fn map_lettermint_error(e: QueryError<LettermintClientError>) -> SendError {
    match e {
        QueryError::Authentication { message, .. } => {
            SendError::Authentication(message.unwrap_or_else(|| "authentication failed".into()))
        }
        QueryError::RateLimit { message, .. } => {
            SendError::RateLimitExceeded(message.unwrap_or_else(|| "rate limit exceeded".into()))
        }
        QueryError::Validation { message, .. } => {
            SendError::InvalidAddress(message.unwrap_or_else(|| "validation failed".into()))
        }
        QueryError::Api {
            status, message, ..
        } if status.is_server_error() => SendError::ServiceUnavailable(
            message.unwrap_or_else(|| format!("server error {status}")),
        ),
        other => SendError::Provider(other.to_string()),
    }
}

/// Maximum batch size for a single Lettermint batch request.
const BATCH_LIMIT: usize = 500;

fn map_batch_item(r: SendEmailResponse) -> BatchItemResult {
    match r.status {
        EmailStatus::Failed => BatchItemResult::Failed(SendError::Provider(format!(
            "message {} failed",
            r.message_id
        ))),
        EmailStatus::HardBounced => {
            BatchItemResult::Failed(SendError::HardBounce(format!("message {}", r.message_id)))
        }
        EmailStatus::SpamComplaint => BatchItemResult::Failed(SendError::SpamComplaint(format!(
            "message {}",
            r.message_id
        ))),
        EmailStatus::Blocked | EmailStatus::PolicyRejected => BatchItemResult::Failed(
            SendError::InactiveRecipient(format!("message {} {}", r.message_id, r.status)),
        ),
        _ => BatchItemResult::Success(SendResult {
            message_id: Some(r.message_id),
        }),
    }
}

pub struct LettermintMailer {
    client: LettermintClient,
}

impl LettermintMailer {
    pub fn new(api_token: impl Into<String>) -> Self {
        Self {
            client: LettermintClient::new(api_token.into()),
        }
    }

    pub fn with_client(client: LettermintClient) -> Self {
        Self { client }
    }
}

fn to_strings(addrs: &[crate::email::Address]) -> Option<Vec<String>> {
    if addrs.is_empty() {
        None
    } else {
        Some(addrs.iter().map(|a| a.to_string()).collect())
    }
}

fn convert_attachment(a: &crate::email::Attachment) -> LmAttachment {
    let att = match &a.content_id {
        Some(cid) => LmAttachment::inline(&a.filename, &a.content, cid),
        None => LmAttachment::new(&a.filename, &a.content),
    };
    if a.content_type.is_empty() {
        att
    } else {
        att.with_content_type(&a.content_type)
    }
}

fn to_request(email: &Email) -> SendEmailRequest {
    let (html, text) = match &email.body {
        Body::Text(t) => (None, Some(t.clone())),
        Body::Html(h) => (Some(h.clone()), None),
        Body::Both { html, text } => (Some(html.clone()), Some(text.clone())),
    };

    SendEmailRequest {
        from: email.from.to_string(),
        to: email.to.iter().map(|a| a.to_string()).collect(),
        subject: email.subject.clone(),
        html,
        text,
        cc: to_strings(&email.cc),
        bcc: to_strings(&email.bcc),
        reply_to: to_strings(&email.reply_to),
        headers: if email.headers.is_empty() {
            None
        } else {
            Some(email.headers.clone())
        },
        attachments: if email.attachments.is_empty() {
            None
        } else {
            Some(email.attachments.iter().map(convert_attachment).collect())
        },
        route: None,
        metadata: if email.metadata.is_empty() {
            None
        } else {
            Some(email.metadata.clone())
        },
        tag: email.tags.first().cloned(),
        idempotency_key: None,
    }
}

#[async_trait]
impl Mailer for LettermintMailer {
    async fn send(&self, email: &Email) -> Result<SendResult, SendError> {
        let req = to_request(email);
        let resp = req
            .execute(&self.client)
            .await
            .map_err(map_lettermint_error)?;

        match map_batch_item(resp) {
            BatchItemResult::Success(result) => Ok(result),
            BatchItemResult::Failed(err) => Err(err),
        }
    }

    async fn batch_send(&self, emails: &[Email]) -> Result<Vec<BatchItemResult>, SendError> {
        if emails.is_empty() {
            return Err(SendError::BatchEmpty);
        }
        if emails.len() > BATCH_LIMIT {
            return Err(SendError::BatchTooLarge {
                count: emails.len(),
                limit: BATCH_LIMIT,
            });
        }

        let requests: Vec<SendEmailRequest> = emails.iter().map(to_request).collect();
        let batch =
            BatchSendRequest::new(requests).map_err(|e| SendError::Provider(e.to_string()))?;

        let responses = batch
            .execute(&self.client)
            .await
            .map_err(map_lettermint_error)?;

        Ok(responses.into_iter().map(map_batch_item).collect())
    }
}
