# Polymail

[![ci](https://github.com/franzos/polymail-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/franzos/polymail-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/polymail.svg)](https://crates.io/crates/polymail)
[![Documentation](https://docs.rs/polymail/badge.svg)](https://docs.rs/polymail)

Unified email sending interface for Rust. Write your email once, send it through any supported provider — swap providers by changing one line.

Currently supported: [Lettermint](https://lettermint.co), [Postmark](https://postmarkapp.com), [SendGrid](https://sendgrid.com).

## Usage

```toml
[dependencies]
polymail = { version = "0.1", features = ["lettermint"] }
tokio = { version = "1", features = ["rt", "macros"] }
```

Lettermint is the default feature, so `features = ["lettermint"]` can be omitted.

### Send an email

```rust,ignore
use polymail::{Email, Body, Mailer};
use polymail::provider::lettermint::LettermintMailer;

#[tokio::main]
async fn main() {
    let mailer = LettermintMailer::new("your-api-token");

    let email = Email::builder("sender@yourdomain.com", "Hello", Body::Text("Hi there!".into()))
        .to("recipient@example.com")
        .build()
        .unwrap();

    let result = mailer.send(&email).await.unwrap();
    println!("Sent: {:?}", result.message_id);
}
```

### HTML + text with all options

```rust,ignore
use polymail::{Email, Body, Address, Attachment, Mailer};
use polymail::provider::lettermint::LettermintMailer;

async fn send_full(mailer: &LettermintMailer) {
    let email = Email::builder(
            Address::with_name("jane@yourdomain.com", "Jane"),
            "Monthly update",
            Body::Both {
                html: "<h1>Update</h1><p>Here's what happened.</p>".into(),
                text: "Here's what happened.".into(),
            },
        )
        .to("user@example.com")
        .cc("team@example.com")
        .bcc("archive@example.com")
        .reply_to("support@yourdomain.com")
        .header("X-Campaign", "monthly-update")
        .attachment(Attachment {
            filename: "report.pdf".into(),
            content: "<base64-encoded-content>".into(),
            content_type: "application/pdf".into(),
            content_id: None,
        })
        .tag("newsletter")
        .metadata("campaign_id", "2025-03")
        .build()
        .unwrap();

    let result = mailer.send(&email).await.unwrap();
    println!("{:?}", result);
}
```

### Batch sending

Providers with native batch support send all emails in a single API call. Others fall back to sequential sends.

```rust,ignore
use polymail::{Email, Body, BatchItemResult, Mailer};
use polymail::provider::lettermint::LettermintMailer;

async fn send_batch(mailer: &LettermintMailer) {
    let emails: Vec<Email> = vec![
        Email::builder("sender@yourdomain.com", "Hello Alice", Body::Text("Hi Alice!".into()))
            .to("alice@example.com")
            .build()
            .unwrap(),
        Email::builder("sender@yourdomain.com", "Hello Bob", Body::Text("Hi Bob!".into()))
            .to("bob@example.com")
            .build()
            .unwrap(),
    ];

    let results = mailer.batch_send(&emails).await.unwrap();
    for (i, result) in results.iter().enumerate() {
        match result {
            BatchItemResult::Success(r) => println!("#{i}: sent {:?}", r.message_id),
            BatchItemResult::Failed(e) => println!("#{i}: failed {e}"),
        }
    }
}
```

### Switching providers

```rust,ignore
use polymail::{Email, Body, Mailer};
use polymail::provider::postmark::PostmarkMailer;

let mailer = PostmarkMailer::new("your-server-token");
// Same Email, same .send() call — just a different mailer.
```

### Fallback across providers

`FallbackMailer` tries providers in order. On transient failures (network issues, rate limits, service outages), it moves to the next provider. On permanent failures (invalid address, hard bounce), it returns immediately — retrying won't help.

```rust,ignore
use polymail::{FallbackMailer, Mailer};
use polymail::provider::lettermint::LettermintMailer;
use polymail::provider::postmark::PostmarkMailer;

let mailer = FallbackMailer::new(vec![
    Box::new(LettermintMailer::new("lettermint-token")),
    Box::new(PostmarkMailer::new("postmark-token")),
]);

// Tries Lettermint first; if it's down, sends through Postmark.
let result = mailer.send(&email).await?;
```

`FallbackMailer` implements `Mailer`, so it works anywhere a single provider does — including `Box<dyn Mailer>`.

Errors that trigger fallback:

| Error | Fallback? | Reason |
|---|---|---|
| `Provider` | yes | Transport failure (network, TLS, timeout) |
| `RateLimitExceeded` | yes | Provider-specific quota, next provider may accept |
| `ServiceUnavailable` | yes | Provider is down |
| `Authentication` | yes | Bad key for this provider, next may work |
| `InvalidAddress` | no | Bad email, will fail everywhere |
| `InactiveRecipient` | no | Recipient-level suppression |
| `SpamComplaint` | no | Recipient-level suppression |
| `HardBounce` | no | Recipient-level suppression |
| `Serialization` | no | Client-side bug |

### Using as a trait object

```rust,ignore
use polymail::Mailer;
use polymail::provider::lettermint::LettermintMailer;

fn get_mailer() -> Box<dyn Mailer> {
    Box::new(LettermintMailer::new("token"))
}
```

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `lettermint` | yes | Lettermint provider |
| `postmark` | no | Postmark provider |
| `sendgrid` | no | SendGrid provider |

Enable multiple providers at once:

```toml
polymail = { version = "0.1", features = ["lettermint", "postmark"] }
```

## Provider capabilities

| Capability | Lettermint | Postmark | SendGrid |
|---|---|---|---|
| Single send | yes | yes | yes |
| Batch send (native) | yes (up to 500) | yes (up to 500) | no (sequential fallback) |
| Attachments | yes | yes | yes |
| Inline attachments | yes | yes | yes |
| Custom headers | yes | yes | yes (per-personalization) |
| Multiple reply-to | yes | first only | first only |
| Tags | first tag | first tag | multiple (categories) |
| Metadata | yes | yes | yes (as custom args) |

## Error handling

Provider-specific errors are mapped to shared `SendError` variants so you can handle common failure modes without matching on providers:

```rust,ignore
use polymail::{Mailer, SendError};

match mailer.send(&email).await {
    Ok(result) => println!("sent: {:?}", result.message_id),
    Err(SendError::RateLimitExceeded(_)) => println!("back off and retry"),
    Err(SendError::Authentication(_)) => println!("check your API key"),
    Err(SendError::InvalidAddress(msg)) => println!("bad address: {msg}"),
    Err(e) => println!("other error: {e}"),
}
```

### Error mapping by provider

| `SendError` | Postmark | Lettermint | SendGrid |
|---|---|---|---|
| `Authentication` | — | HTTP 401/403 | HTTP 401/403 |
| `InvalidAddress` | error code 300 | HTTP 422 (validation) | HTTP 400 |
| `InactiveRecipient` | error code 406 | batch status | — |
| `SpamComplaint` | error code 409 | batch status | — |
| `HardBounce` | error code 422 | batch status | — |
| `RateLimitExceeded` | error code 429 | HTTP 429 | HTTP 429 |
| `ServiceUnavailable` | error codes 500–504 | HTTP 5xx | HTTP 500–504 |
| `Provider` | transport errors | transport/parse errors | transport/parse errors |
| `Api` | other error codes | other HTTP errors | other HTTP errors |

## Testing

```sh
cargo test --all-features
```

## License

Dual-licensed under MIT or Apache 2.0.
