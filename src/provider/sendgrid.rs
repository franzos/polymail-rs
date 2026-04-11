use async_trait::async_trait;
use sendgrid::error::SendgridError;
use sendgrid::v3;

use crate::email::{Body, Email};
use crate::error::SendError;
use crate::mailer::{Mailer, SendResult};

fn map_sendgrid_error(e: SendgridError) -> SendError {
    match e {
        SendgridError::RequestNotSuccessful(ref rns) => {
            let status = rns.status.as_u16();
            let body = &rns.body;
            match status {
                401 | 403 => SendError::Authentication(body.clone()),
                429 => SendError::RateLimitExceeded(body.clone()),
                400 => SendError::InvalidAddress(body.clone()),
                500 | 502 | 503 | 504 => SendError::ServiceUnavailable(body.clone()),
                _ => SendError::Api {
                    status,
                    message: body.clone(),
                },
            }
        }
        SendgridError::InvalidHeader(_) => SendError::Authentication(e.to_string()),
        _ => SendError::Provider(e.to_string()),
    }
}

pub struct SendgridMailer {
    sender: v3::Sender<'static>,
}

impl SendgridMailer {
    /// Create a new SendGrid mailer.
    ///
    /// The API key is consumed during client construction to set HTTP headers
    /// and is not stored in the `Sender` struct.
    pub fn new(api_key: impl Into<String>) -> Self {
        let key = api_key.into();
        Self {
            sender: v3::Sender::new(&key, None),
        }
    }

    pub fn with_sender(sender: v3::Sender<'static>) -> Self {
        Self { sender }
    }
}

fn to_sg_email(addr: &crate::email::Address) -> v3::Email<'_> {
    let e = v3::Email::new(&addr.email);
    match &addr.name {
        Some(name) => e.set_name(name),
        None => e,
    }
}

#[async_trait]
impl Mailer for SendgridMailer {
    async fn send(&self, email: &Email) -> Result<SendResult, SendError> {
        let from = to_sg_email(&email.from);
        let subject = &email.subject;
        let to_emails: Vec<v3::Email<'_>> = email.to.iter().map(to_sg_email).collect();

        let mut personalization = if to_emails.len() == 1 {
            v3::Personalization::new(to_emails.into_iter().next().unwrap())
        } else {
            v3::Personalization::new_many(to_emails)
        };

        for addr in &email.cc {
            personalization = personalization.add_cc(to_sg_email(addr));
        }

        for addr in &email.bcc {
            personalization = personalization.add_bcc(to_sg_email(addr));
        }

        if !email.headers.is_empty() {
            let sg_headers: v3::SGMap<'_> = email
                .headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            personalization = personalization.add_headers(&sg_headers);
        }

        if !email.metadata.is_empty() {
            let sg_args: v3::SGMap<'_> = email
                .metadata
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            personalization = personalization.add_custom_args(&sg_args);
        }

        let mut msg = v3::Message::new(from)
            .set_subject(subject)
            .add_personalization(personalization);

        match &email.body {
            Body::Text(t) => {
                msg = msg.add_content(
                    v3::Content::new()
                        .set_content_type("text/plain")
                        .set_value(t),
                );
            }
            Body::Html(h) => {
                msg = msg.add_content(
                    v3::Content::new()
                        .set_content_type("text/html")
                        .set_value(h),
                );
            }
            Body::Both { html, text } => {
                msg = msg
                    .add_content(
                        v3::Content::new()
                            .set_content_type("text/plain")
                            .set_value(text),
                    )
                    .add_content(
                        v3::Content::new()
                            .set_content_type("text/html")
                            .set_value(html),
                    );
            }
        }

        if let Some(reply_to) = email.reply_to.first() {
            msg = msg.set_reply_to(to_sg_email(reply_to));
        }

        for att in &email.attachments {
            let mut sg_att = v3::Attachment::new()
                .set_base64_content(&att.content)
                .set_filename(&att.filename)
                .set_mime_type(&att.content_type);
            if let Some(cid) = &att.content_id {
                sg_att = sg_att
                    .set_content_idm(cid)
                    .set_disposition(v3::Disposition::Inline);
            }
            msg = msg.add_attachment(sg_att);
        }

        for tag in &email.tags {
            msg = msg.add_category(tag);
        }

        let resp = self.sender.send(&msg).await.map_err(map_sendgrid_error)?;

        let message_id = resp
            .headers()
            .get("X-Message-Id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        Ok(SendResult { message_id })
    }
}
