use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_config::default_provider::credentials::DefaultCredentialsChain;
use aws_credential_types::provider::{ProvideCredentials, future};
use aws_sdk_sesv2::Client;
use aws_sdk_sesv2::config::http::HttpResponse;
use aws_sdk_sesv2::config::{Credentials, Region};
use aws_sdk_sesv2::error::{DisplayErrorContext, SdkError};
use aws_sdk_sesv2::operation::send_email::SendEmailError;
use aws_sdk_sesv2::primitives::Blob;
use aws_sdk_sesv2::types::{Destination, EmailContent, MessageTag, RawMessage};
use tokio::sync::OnceCell;

use crate::email::{Address, Email};
use crate::error::SendError;
use crate::mailer::{Mailer, SendResult};
use crate::mime::to_message;

/// Amazon SES (SESv2 `SendEmail` API).
///
/// Messages go out as raw MIME, so attachments, inline images and custom
/// headers behave as with the SMTP provider. `metadata` maps to SES message
/// tags; `tags` has no SES equivalent and is ignored.
pub struct SesMailer {
    client: Client,
    configuration_set: Option<String>,
}

impl SesMailer {
    /// Region and credentials from the AWS default chain (env, profile, SSO,
    /// web identity, ECS, IMDS).
    pub async fn from_env() -> Self {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        Self::with_client(Client::new(&config))
    }

    /// Static access key credentials.
    pub fn new(
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        let creds = Credentials::new(access_key_id, secret_access_key, None, None, "polymail");
        Self::with_credentials(Region::new(region.into()), creds)
    }

    /// Fixed region, credentials resolved through the AWS default chain on
    /// first send.
    pub fn with_default_credentials(region: impl Into<String>) -> Self {
        let region = Region::new(region.into());
        let creds = LazyDefaultChain {
            region: region.clone(),
            chain: OnceCell::new(),
        };
        Self::with_credentials(region, creds)
    }

    /// Escape hatch for a fully-configured SDK client.
    pub fn with_client(client: Client) -> Self {
        Self {
            client,
            configuration_set: None,
        }
    }

    /// Send through this SES configuration set (event publishing, IP pools).
    pub fn configuration_set(mut self, name: impl Into<String>) -> Self {
        self.configuration_set = Some(name.into());
        self
    }

    fn with_credentials(region: Region, creds: impl ProvideCredentials + 'static) -> Self {
        let config = aws_sdk_sesv2::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(region)
            .credentials_provider(creds)
            .build();
        Self::with_client(Client::from_conf(config))
    }
}

/// `DefaultCredentialsChain` can only be built async; defer it to the first
/// request so `SesMailer` can be constructed from sync code.
#[derive(Debug)]
struct LazyDefaultChain {
    region: Region,
    chain: OnceCell<DefaultCredentialsChain>,
}

impl ProvideCredentials for LazyDefaultChain {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(async move {
            let chain = self
                .chain
                .get_or_init(|| {
                    DefaultCredentialsChain::builder()
                        .region(self.region.clone())
                        .build()
                })
                .await;
            chain.provide_credentials().await
        })
    }
}

fn emails(addrs: &[Address]) -> Option<Vec<String>> {
    if addrs.is_empty() {
        None
    } else {
        Some(addrs.iter().map(|a| a.email.clone()).collect())
    }
}

fn map_ses_error(e: SdkError<SendEmailError, HttpResponse>) -> SendError {
    let msg = DisplayErrorContext(&e).to_string();
    let SdkError::ServiceError(ctx) = &e else {
        return SendError::Provider(msg);
    };
    match ctx.err() {
        SendEmailError::TooManyRequestsException(_) | SendEmailError::LimitExceededException(_) => {
            SendError::RateLimitExceeded(msg)
        }
        SendEmailError::AccountSuspendedException(_)
        | SendEmailError::SendingPausedException(_) => SendError::ServiceUnavailable(msg),
        SendEmailError::BadRequestException(_) => SendError::InvalidAddress(msg),
        SendEmailError::MessageRejected(_)
        | SendEmailError::MailFromDomainNotVerifiedException(_) => SendError::Api {
            status: 400,
            message: msg,
        },
        SendEmailError::NotFoundException(_) => SendError::Api {
            status: 404,
            message: msg,
        },
        _ => map_unhandled(&e, msg),
    }
}

fn map_unhandled(e: &SdkError<SendEmailError, HttpResponse>, msg: String) -> SendError {
    let status = e.raw_response().map(|r| r.status().as_u16()).unwrap_or(0);
    match status {
        401 | 403 => SendError::Authentication(msg),
        429 => SendError::RateLimitExceeded(msg),
        500..=599 => SendError::ServiceUnavailable(msg),
        _ => SendError::Api {
            status,
            message: msg,
        },
    }
}

#[async_trait]
impl Mailer for SesMailer {
    async fn send(&self, email: &Email) -> Result<SendResult, SendError> {
        // Bcc is stripped from the MIME headers, so every recipient goes in the
        // envelope explicitly.
        let raw = to_message(email)?.formatted();
        let destination = Destination::builder()
            .set_to_addresses(emails(&email.to))
            .set_cc_addresses(emails(&email.cc))
            .set_bcc_addresses(emails(&email.bcc))
            .build();
        let raw = RawMessage::builder()
            .data(Blob::new(raw))
            .build()
            .map_err(|e| SendError::Serialization(e.to_string()))?;

        let mut tags: Vec<_> = email.metadata.iter().collect();
        tags.sort();
        let tags = tags
            .into_iter()
            .map(|(k, v)| MessageTag::builder().name(k).value(v).build())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SendError::Serialization(e.to_string()))?;

        let out = self
            .client
            .send_email()
            .destination(destination)
            .content(EmailContent::builder().raw(raw).build())
            .set_email_tags((!tags.is_empty()).then_some(tags))
            .set_configuration_set_name(self.configuration_set.clone())
            .send()
            .await
            .map_err(map_ses_error)?;

        Ok(SendResult {
            message_id: out.message_id().map(str::to_owned),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_sesv2::config::retry::RetryConfig;
    use aws_smithy_http_client::test_util::{CaptureRequestReceiver, capture_request};
    use aws_smithy_types::body::SdkBody;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    use crate::email::Body;

    fn mailer(
        status: u16,
        error_type: Option<&str>,
        body: &str,
    ) -> (SesMailer, CaptureRequestReceiver) {
        let mut resp = http::Response::builder().status(status);
        if let Some(t) = error_type {
            resp = resp.header("x-amzn-errortype", t);
        }
        let (http_client, rx) = capture_request(Some(resp.body(SdkBody::from(body)).unwrap()));
        let config = aws_sdk_sesv2::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("eu-central-1"))
            .credentials_provider(Credentials::for_tests())
            .retry_config(RetryConfig::disabled())
            .http_client(http_client)
            .build();
        (SesMailer::with_client(Client::from_conf(config)), rx)
    }

    fn email() -> Email {
        Email::builder(
            Address::with_name("sender@example.com", "Sender"),
            "Hello SES",
            Body::Text("plain body".into()),
        )
        .to("to@example.com")
        .cc("cc@example.com")
        .bcc("hidden@example.com")
        .tag("ignored-tag")
        .metadata("campaign", "launch")
        .build()
        .unwrap()
    }

    async fn send_err(status: u16, error_type: &str) -> SendError {
        let (m, _rx) = mailer(status, Some(error_type), r#"{"message":"nope"}"#);
        m.send(&email()).await.unwrap_err()
    }

    #[tokio::test]
    async fn sends_raw_message_with_envelope_and_tags() {
        let (m, rx) = mailer(200, None, r#"{"MessageId":"ses-msg-1"}"#);
        let m = m.configuration_set("tracking");
        let result = m.send(&email()).await.unwrap();
        assert_eq!(result.message_id.as_deref(), Some("ses-msg-1"));

        let req = rx.expect_request();
        assert_eq!(
            req.uri(),
            "https://email.eu-central-1.amazonaws.com/v2/email/outbound-emails"
        );
        let auth = req.headers().get("authorization").unwrap();
        assert!(auth.starts_with("AWS4-HMAC-SHA256"));

        let body: serde_json::Value = serde_json::from_slice(req.body().bytes().unwrap()).unwrap();
        assert_eq!(
            body["Destination"]["ToAddresses"],
            serde_json::json!(["to@example.com"])
        );
        assert_eq!(
            body["Destination"]["CcAddresses"],
            serde_json::json!(["cc@example.com"])
        );
        assert_eq!(
            body["Destination"]["BccAddresses"],
            serde_json::json!(["hidden@example.com"])
        );
        assert_eq!(
            body["EmailTags"],
            serde_json::json!([{"Name": "campaign", "Value": "launch"}])
        );
        assert_eq!(body["ConfigurationSetName"], "tracking");

        let raw = STANDARD
            .decode(body["Content"]["Raw"]["Data"].as_str().unwrap())
            .unwrap();
        let raw = String::from_utf8(raw).unwrap();
        assert!(raw.contains("Subject: Hello SES"));
        assert!(raw.contains("From: Sender <sender@example.com>"), "{raw}");
        assert!(!raw.contains("hidden@example.com"));
        assert!(!raw.contains("ignored-tag"));
    }

    #[tokio::test]
    async fn no_tags_or_configuration_set_when_unset() {
        let (m, rx) = mailer(200, None, r#"{"MessageId":"x"}"#);
        let mut e = email();
        e.metadata.clear();
        m.send(&e).await.unwrap();
        let req = rx.expect_request();
        let body: serde_json::Value = serde_json::from_slice(req.body().bytes().unwrap()).unwrap();
        assert!(body.get("EmailTags").is_none());
        assert!(body.get("ConfigurationSetName").is_none());
    }

    #[tokio::test]
    async fn throttling_is_rate_limit() {
        let e = send_err(429, "TooManyRequestsException").await;
        assert!(matches!(e, SendError::RateLimitExceeded(_)));
        assert!(e.is_retryable());
    }

    #[tokio::test]
    async fn quota_is_rate_limit() {
        let e = send_err(400, "LimitExceededException").await;
        assert!(matches!(e, SendError::RateLimitExceeded(_)));
    }

    #[tokio::test]
    async fn paused_account_is_unavailable() {
        let e = send_err(400, "SendingPausedException").await;
        assert!(matches!(e, SendError::ServiceUnavailable(_)));
    }

    #[tokio::test]
    async fn rejected_message_is_permanent() {
        let e = send_err(400, "MessageRejected").await;
        assert!(matches!(e, SendError::Api { status: 400, .. }));
        assert!(!e.is_retryable());
    }

    #[tokio::test]
    async fn bad_request_is_invalid_address() {
        let e = send_err(400, "BadRequestException").await;
        assert!(matches!(e, SendError::InvalidAddress(_)));
    }

    #[tokio::test]
    async fn unrecognized_client_is_authentication() {
        let e = send_err(403, "UnrecognizedClientException").await;
        assert!(matches!(e, SendError::Authentication(_)));
    }

    #[tokio::test]
    async fn server_error_is_unavailable() {
        let e = send_err(503, "InternalFailure").await;
        assert!(matches!(e, SendError::ServiceUnavailable(_)));
    }

    #[test]
    fn constructs_without_runtime() {
        let _ = SesMailer::new("eu-central-1", "AKID", "secret");
        let _ = SesMailer::with_default_credentials("eu-central-1").configuration_set("cs");
    }
}
