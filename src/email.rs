use std::collections::HashMap;
use std::fmt;

use crate::error::SendError;

/// RFC 5322 special characters that require quoting in a display name.
const RFC5322_SPECIALS: &[char] = &[
    '(', ')', '<', '>', '[', ']', ':', ';', '@', '\\', ',', '.', '"',
];

/// An email address with an optional display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub email: String,
    pub name: Option<String>,
}

impl Address {
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            name: None,
        }
    }

    pub fn with_name(email: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            name: Some(name.into()),
        }
    }
}

impl fmt::Display for Address {
    /// RFC 5322 formatted: `"Name" <email>` or just `email`.
    /// Display names containing special characters are quoted.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            Some(name) if name.contains(RFC5322_SPECIALS) => {
                let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
                write!(f, "\"{escaped}\" <{}>", self.email)
            }
            Some(name) => write!(f, "{name} <{}>", self.email),
            None => write!(f, "{}", self.email),
        }
    }
}

impl<S: Into<String>> From<S> for Address {
    fn from(email: S) -> Self {
        Self::new(email)
    }
}

/// The email body — text, html, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Text(String),
    Html(String),
    Both { html: String, text: String },
}

/// A file attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub filename: String,
    /// Base64-encoded content.
    pub content: String,
    pub content_type: String,
    /// For inline images referenced via `cid:` in HTML.
    pub content_id: Option<String>,
}

/// A fully-described email, ready to hand off to any provider.
#[derive(Debug, Clone)]
pub struct Email {
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub subject: String,
    pub body: Body,
    pub headers: HashMap<String, String>,
    pub attachments: Vec<Attachment>,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, String>,
}

impl Email {
    pub fn builder(
        from: impl Into<Address>,
        subject: impl Into<String>,
        body: Body,
    ) -> EmailBuilder {
        EmailBuilder {
            from: from.into(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            subject: subject.into(),
            body,
            headers: HashMap::new(),
            attachments: Vec::new(),
            tags: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}

pub struct EmailBuilder {
    from: Address,
    to: Vec<Address>,
    cc: Vec<Address>,
    bcc: Vec<Address>,
    reply_to: Vec<Address>,
    subject: String,
    body: Body,
    headers: HashMap<String, String>,
    attachments: Vec<Attachment>,
    tags: Vec<String>,
    metadata: HashMap<String, String>,
}

impl EmailBuilder {
    pub fn to(mut self, addr: impl Into<Address>) -> Self {
        self.to.push(addr.into());
        self
    }

    pub fn cc(mut self, addr: impl Into<Address>) -> Self {
        self.cc.push(addr.into());
        self
    }

    pub fn bcc(mut self, addr: impl Into<Address>) -> Self {
        self.bcc.push(addr.into());
        self
    }

    pub fn reply_to(mut self, addr: impl Into<Address>) -> Self {
        self.reply_to.push(addr.into());
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn attachment(mut self, att: Attachment) -> Self {
        self.attachments.push(att);
        self
    }

    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> Result<Email, SendError> {
        if self.to.is_empty() {
            return Err(SendError::InvalidAddress(
                "at least one recipient required".into(),
            ));
        }

        Ok(Email {
            from: self.from,
            to: self.to,
            cc: self.cc,
            bcc: self.bcc,
            reply_to: self.reply_to,
            subject: self.subject,
            body: self.body,
            headers: self.headers,
            attachments: self.attachments,
            tags: self.tags,
            metadata: self.metadata,
        })
    }
}
