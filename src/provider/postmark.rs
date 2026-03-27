use async_trait::async_trait;
use postmark::Query;
use postmark::api::Body as PostmarkBody;
use postmark::api::email::{
    Attachment as PmAttachment, Header as PmHeader, SendEmailBatchRequest, SendEmailRequest,
};
use postmark::reqwest::PostmarkClient;

use crate::email::{Body, Email};
use crate::error::SendError;
use crate::mailer::{BatchItemResult, Mailer, SendResult};

/// Postmark batch limit (500 per API docs).
const BATCH_LIMIT: usize = 500;

/// Map a Postmark error_code + message to the appropriate SendError variant.
fn map_postmark_error(error_code: i64, message: String) -> SendError {
    match error_code {
        300 => SendError::InvalidAddress(message),
        406 => SendError::InactiveRecipient(message),
        409 => SendError::SpamComplaint(message),
        422 => SendError::HardBounce(message),
        429 => SendError::RateLimitExceeded(message),
        500 | 502 | 503 | 504 => SendError::ServiceUnavailable(message),
        _ => SendError::Api {
            status: u16::try_from(error_code).unwrap_or(0),
            message,
        },
    }
}

pub struct PostmarkMailer {
    client: PostmarkClient,
}

impl PostmarkMailer {
    pub fn new(server_token: impl Into<String>) -> Self {
        Self {
            client: PostmarkClient::builder()
                .server_token(server_token.into())
                .build(),
        }
    }

    pub fn with_client(client: PostmarkClient) -> Self {
        Self { client }
    }
}

fn join_addresses(addrs: &[crate::email::Address]) -> Option<String> {
    if addrs.is_empty() {
        None
    } else {
        Some(
            addrs
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

fn to_request(email: &Email) -> SendEmailRequest {
    let body = match &email.body {
        Body::Text(t) => PostmarkBody::text(t.clone()),
        Body::Html(h) => PostmarkBody::html(h.clone()),
        Body::Both { html, text } => PostmarkBody::html_and_text(html.clone(), text.clone()),
    };

    SendEmailRequest {
        from: email.from.to_string(),
        to: email
            .to
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        body,
        subject: Some(email.subject.clone()),
        cc: join_addresses(&email.cc),
        bcc: join_addresses(&email.bcc),
        reply_to: email.reply_to.first().map(|a| a.to_string()),
        tag: email.tags.first().cloned(),
        headers: if email.headers.is_empty() {
            None
        } else {
            Some(
                email
                    .headers
                    .iter()
                    .map(|(k, v)| PmHeader {
                        name: k.clone(),
                        value: v.clone(),
                    })
                    .collect(),
            )
        },
        attachments: if email.attachments.is_empty() {
            None
        } else {
            Some(
                email
                    .attachments
                    .iter()
                    .map(|a| PmAttachment {
                        name: a.filename.clone(),
                        content: a.content.clone(),
                        content_type: a.content_type.clone(),
                        content_id: a.content_id.clone(),
                    })
                    .collect(),
            )
        },
        metadata: if email.metadata.is_empty() {
            None
        } else {
            Some(email.metadata.clone())
        },
        track_opens: None,
        track_links: None,
        message_stream: None,
    }
}

#[async_trait]
impl Mailer for PostmarkMailer {
    async fn send(&self, email: &Email) -> Result<SendResult, SendError> {
        let req = to_request(email);
        let resp = req
            .execute(&self.client)
            .await
            .map_err(|e| SendError::Provider(e.to_string()))?;

        if resp.error_code != 0 {
            return Err(map_postmark_error(resp.error_code, resp.message));
        }

        Ok(SendResult {
            message_id: resp.message_id,
        })
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

        let batch: SendEmailBatchRequest = emails.iter().map(to_request).collect();
        let responses = batch
            .execute(&self.client)
            .await
            .map_err(|e| SendError::Provider(e.to_string()))?;

        Ok(responses
            .into_iter()
            .map(|r| {
                if r.error_code != 0 {
                    BatchItemResult::Failed(map_postmark_error(r.error_code, r.message))
                } else {
                    BatchItemResult::Success(SendResult {
                        message_id: r.message_id,
                    })
                }
            })
            .collect())
    }
}
