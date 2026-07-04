use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use lettre::message::header::{ContentType, HeaderName, HeaderValue};
use lettre::message::{Attachment as LettreAttachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::Error as SmtpError;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{
    Address as LettreAddress, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};

use crate::email::{Body, Email};
use crate::error::SendError;
use crate::mailer::{Mailer, SendResult};

/// Transport security for the SMTP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpTls {
    /// Plaintext, no encryption (e.g. mailcrab on 1025).
    None,
    /// Connect plaintext then upgrade via STARTTLS (typically port 587).
    StartTls,
    /// TLS from the first byte (typically port 465).
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

fn has_crlf(s: &str) -> bool {
    s.contains('\r') || s.contains('\n')
}

fn to_mailbox(addr: &crate::email::Address) -> Result<Mailbox, SendError> {
    // A CR/LF in the display name makes lettre's header encoder panic on send.
    if addr.name.as_deref().is_some_and(has_crlf) {
        return Err(SendError::InvalidAddress(format!(
            "{}: display name contains CR/LF",
            addr.email
        )));
    }
    let parsed = addr
        .email
        .parse::<LettreAddress>()
        .map_err(|e| SendError::InvalidAddress(format!("{}: {e}", addr.email)))?;
    Ok(Mailbox::new(addr.name.clone(), parsed))
}

enum Part {
    Single(SinglePart),
    Multi(MultiPart),
}

fn add_part(mp: MultiPart, part: Part) -> MultiPart {
    match part {
        Part::Single(sp) => mp.singlepart(sp),
        Part::Multi(m) => mp.multipart(m),
    }
}

fn body_part(body: &Body) -> Part {
    match body {
        Body::Text(t) => Part::Single(SinglePart::plain(t.clone())),
        Body::Html(h) => Part::Single(SinglePart::html(h.clone())),
        Body::Both { html, text } => Part::Multi(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(text.clone()))
                .singlepart(SinglePart::html(html.clone())),
        ),
    }
}

fn attachment_content_type(raw: &str) -> ContentType {
    ContentType::parse(raw)
        .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("valid mime"))
}

fn attachment_part(att: &crate::email::Attachment) -> Result<SinglePart, SendError> {
    let bytes = STANDARD
        .decode(&att.content)
        .map_err(|e| SendError::Serialization(e.to_string()))?;
    let ct = attachment_content_type(&att.content_type);
    let sp = match &att.content_id {
        Some(cid) => LettreAttachment::new_inline(cid.clone()).body(bytes, ct),
        None => LettreAttachment::new(att.filename.clone()).body(bytes, ct),
    };
    Ok(sp)
}

fn to_message(email: &Email) -> Result<Message, SendError> {
    let mut builder = Message::builder().from(to_mailbox(&email.from)?);
    for a in &email.to {
        builder = builder.to(to_mailbox(a)?);
    }
    for a in &email.cc {
        builder = builder.cc(to_mailbox(a)?);
    }
    for a in &email.bcc {
        builder = builder.bcc(to_mailbox(a)?);
    }
    if let Some(rt) = email.reply_to.first() {
        builder = builder.reply_to(to_mailbox(rt)?);
    }
    builder = builder.subject(email.subject.clone());
    for (k, v) in &email.headers {
        // lettre's HeaderName accepts CR/LF, which would inject arbitrary
        // headers (e.g. a hidden Bcc); reject them here.
        if has_crlf(k) || has_crlf(v) {
            return Err(SendError::Serialization(format!(
                "header {k:?} contains CR/LF"
            )));
        }
        let name = HeaderName::new_from_ascii(k.clone())
            .map_err(|e| SendError::Serialization(e.to_string()))?;
        builder = builder.raw_header(HeaderValue::new(name, v.clone()));
    }

    let (inline, regular): (Vec<_>, Vec<_>) = email
        .attachments
        .iter()
        .partition(|a| a.content_id.is_some());

    let content = if inline.is_empty() {
        body_part(&email.body)
    } else {
        let mut mp = add_part(MultiPart::related().build(), body_part(&email.body));
        for a in &inline {
            mp = mp.singlepart(attachment_part(a)?);
        }
        Part::Multi(mp)
    };

    let final_part = if regular.is_empty() {
        content
    } else {
        let mut mp = add_part(MultiPart::mixed().build(), content);
        for a in &regular {
            mp = mp.singlepart(attachment_part(a)?);
        }
        Part::Multi(mp)
    };

    let msg = match final_part {
        Part::Single(sp) => builder.singlepart(sp),
        Part::Multi(m) => builder.multipart(m),
    };
    msg.map_err(|e| SendError::Serialization(e.to_string()))
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
    use crate::email::{Address, Attachment};

    fn body_text() -> Body {
        Body::Text("plain body".into())
    }

    fn base_email(body: Body) -> Email {
        Email::builder(Address::new("sender@example.com"), "Subject", body)
            .to("recipient@example.com")
            .build()
            .unwrap()
    }

    fn formatted(email: &Email) -> String {
        String::from_utf8(to_message(email).unwrap().formatted()).unwrap()
    }

    #[test]
    fn text_body_is_plain() {
        let out = formatted(&base_email(Body::Text("hello text".into())));
        assert!(out.contains("Content-Type: text/plain"));
        assert!(out.contains("hello text"));
        assert!(!out.contains("multipart"));
    }

    #[test]
    fn html_body_is_html() {
        let out = formatted(&base_email(Body::Html("<p>hi</p>".into())));
        assert!(out.contains("Content-Type: text/html"));
        assert!(out.contains("<p>hi</p>"));
    }

    #[test]
    fn both_body_is_alternative() {
        let out = formatted(&base_email(Body::Both {
            html: "<p>rich</p>".into(),
            text: "poor".into(),
        }));
        assert!(out.contains("multipart/alternative"));
        assert!(out.contains("Content-Type: text/plain"));
        assert!(out.contains("Content-Type: text/html"));
    }

    #[test]
    fn regular_attachment_is_mixed_and_decoded() {
        // "aGVsbG8=" is base64 for "hello"; decoded before handoff, so "hello" appears and the base64 does not.
        let email = Email::builder(Address::new("s@example.com"), "S", body_text())
            .to("r@example.com")
            .attachment(Attachment {
                filename: "note.txt".into(),
                content: "aGVsbG8=".into(),
                content_type: "text/plain".into(),
                content_id: None,
            })
            .build()
            .unwrap();
        let out = formatted(&email);
        assert!(out.contains("multipart/mixed"));
        assert!(out.contains("note.txt"));
        assert!(out.contains("hello"));
        assert!(!out.contains("aGVsbG8="));
    }

    #[test]
    fn inline_attachment_is_related() {
        let email = Email::builder(Address::new("s@example.com"), "S", body_text())
            .to("r@example.com")
            .attachment(Attachment {
                filename: "logo.png".into(),
                content: "aGVsbG8=".into(),
                content_type: "image/png".into(),
                content_id: Some("logo123".into()),
            })
            .build()
            .unwrap();
        let out = formatted(&email);
        assert!(out.contains("multipart/related"));
        assert!(out.contains("logo123"));
    }

    #[test]
    fn inline_and_regular_nest_related_in_mixed() {
        let email = Email::builder(Address::new("s@example.com"), "S", body_text())
            .to("r@example.com")
            .attachment(Attachment {
                filename: "logo.png".into(),
                content: "aGVsbG8=".into(),
                content_type: "image/png".into(),
                content_id: Some("logo123".into()),
            })
            .attachment(Attachment {
                filename: "note.txt".into(),
                content: "aGVsbG8=".into(),
                content_type: "text/plain".into(),
                content_id: None,
            })
            .build()
            .unwrap();
        let out = formatted(&email);
        assert!(out.contains("multipart/mixed"));
        assert!(out.contains("multipart/related"));
        let mixed = out.find("multipart/mixed").unwrap();
        let related = out.find("multipart/related").unwrap();
        assert!(mixed < related);
    }

    #[test]
    fn explicit_headers_present_tags_metadata_absent() {
        let email = Email::builder(Address::new("s@example.com"), "S", body_text())
            .to("r@example.com")
            .header("X-Custom", "custom-value")
            .tag("secret-tag-xyz")
            .metadata("secret-meta-key", "secret-meta-value")
            .build()
            .unwrap();
        let out = formatted(&email);
        assert!(out.contains("X-Custom: custom-value"));
        assert!(!out.contains("secret-tag-xyz"));
        assert!(!out.contains("secret-meta-key"));
        assert!(!out.contains("secret-meta-value"));
    }

    #[test]
    fn invalid_address_errors() {
        let email = base_email(body_text());
        let mut bad = email;
        bad.from = Address::new("not-an-email");
        assert!(matches!(
            to_message(&bad),
            Err(SendError::InvalidAddress(_))
        ));
    }

    #[test]
    fn bad_base64_attachment_errors() {
        let email = Email::builder(Address::new("s@example.com"), "S", body_text())
            .to("r@example.com")
            .attachment(Attachment {
                filename: "note.txt".into(),
                content: "!!!not base64!!!".into(),
                content_type: "text/plain".into(),
                content_id: None,
            })
            .build()
            .unwrap();
        assert!(matches!(
            to_message(&email),
            Err(SendError::Serialization(_))
        ));
    }

    #[test]
    fn invalid_header_name_errors() {
        let email = Email::builder(Address::new("s@example.com"), "S", body_text())
            .to("r@example.com")
            .header("Bad Name", "value")
            .build()
            .unwrap();
        assert!(matches!(
            to_message(&email),
            Err(SendError::Serialization(_))
        ));
    }

    #[test]
    fn crlf_in_header_is_rejected() {
        let email = Email::builder(Address::new("s@example.com"), "S", body_text())
            .to("r@example.com")
            .header("X-Foo\r\nBcc: evil@example.com", "x")
            .build()
            .unwrap();
        assert!(matches!(
            to_message(&email),
            Err(SendError::Serialization(_))
        ));
    }

    #[test]
    fn crlf_in_display_name_is_rejected() {
        let email = Email::builder(
            Address::with_name("s@example.com", "Bad\r\nName"),
            "S",
            body_text(),
        )
        .to("r@example.com")
        .build()
        .unwrap();
        assert!(matches!(
            to_message(&email),
            Err(SendError::InvalidAddress(_))
        ));
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
