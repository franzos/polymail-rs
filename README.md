# Polymail

[![ci](https://github.com/franzos/polymail-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/franzos/polymail-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/polymail.svg)](https://crates.io/crates/polymail)
[![Documentation](https://docs.rs/polymail/badge.svg)](https://docs.rs/polymail)

Unified email sending interface for Rust. Write your email once, send it through any supported provider — swap providers by changing one line.

Currently supported: [Lettermint](https://lettermint.co), [Postmark](https://postmarkapp.com), [SendGrid](https://sendgrid.com), and any SMTP server (via [lettre](https://github.com/lettre/lettre)).

## Usage

```toml
[dependencies]
polymail = { version = "0.1" }
tokio = { version = "1", features = ["rt", "macros"] }
```

The Lettermint provider on reqwest 0.13 is the default, so no `features` are needed for it. See [Features](#features) to switch to reqwest 0.12 or enable other providers.

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

### Custom HTTP client

The Lettermint mailer can take a caller-supplied reqwest client, so you can share a connection pool or set your own timeouts, proxy, or TLS. The client version must match the backend this build selected. Use the re-exported `backend` module (`provider::lettermint::backend`) to name the right type without pinning a version yourself; it resolves to reqwest 0.13 by default, or 0.12 with the `lettermint-reqwest-012` feature.

```rust,ignore
use polymail::provider::lettermint::{LettermintMailer, backend};

let http = backend::Client::builder()
    .timeout(std::time::Duration::from_secs(60))
    .build()
    .unwrap();

let mailer = LettermintMailer::with_reqwest_client("your-api-token", http);
```

The other providers can't take a bare `reqwest::Client`: SendGrid requires the auth header baked into the client at build time (build the `Sender` yourself and pass it to `SendgridMailer::with_sender`), Postmark keeps its reqwest client private (pass a built `PostmarkClient` to `PostmarkMailer::with_client`), and SMTP uses lettre, not reqwest.

### SMTP (any server)

The `smtp` feature sends through any SMTP server via lettre. Pick the transport security with `SmtpTls` (`Implicit` for port 465, `StartTls` for 587, `None` for plaintext).

```rust,ignore
use polymail::{Email, Body, Mailer};
use polymail::provider::smtp::{SmtpMailer, SmtpTls};

async fn send_smtp() {
    // Production relay with STARTTLS + auth:
    let mailer = SmtpMailer::builder("smtp.example.com")
        .tls(SmtpTls::StartTls)
        .credentials("user", "pass")
        .build()
        .unwrap();

    // Or a local sink for testing (plaintext, no auth), e.g. mailcrab or MailHog:
    let mailer = SmtpMailer::plaintext("localhost", 1025);

    let email = Email::builder("sender@example.com", "Hello", Body::Text("Hi there!".into()))
        .to("recipient@example.com")
        .build()
        .unwrap();

    mailer.send(&email).await.unwrap();
}
```

For custom trust roots, pool tuning, or alternate auth mechanisms, build a `lettre` `AsyncSmtpTransport` yourself and pass it to `SmtpMailer::with_transport`.

### Fallback across providers

`FallbackMailer` tries providers in order. On transient failures (network issues, rate limits, service outages), it moves to the next provider. On permanent failures (invalid address, hard bounce), it returns immediately — retrying won't help.

```rust,ignore
use polymail::{FallbackMailer, Mailer};
use polymail::provider::lettermint::LettermintMailer;
use polymail::provider::smtp::{SmtpMailer, SmtpTls};

let mailer = FallbackMailer::new(vec![
    Box::new(LettermintMailer::new("lettermint-token")),
    Box::new(
        SmtpMailer::builder("smtp.example.com")
            .tls(SmtpTls::StartTls)
            .credentials("user", "pass")
            .build()?,
    ),
]);

// Tries the Lettermint API first; if it's down (or rate-limited),
// falls back to your own SMTP relay.
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
| `lettermint-reqwest-013` | yes | Lettermint provider on reqwest 0.13 (rustls) |
| `lettermint-reqwest-012` | no | Lettermint provider on reqwest 0.12 (rustls) |
| `lettermint` | no | Lettermint provider without a reqwest backend (pick one of the two above) |
| `postmark` | no | Postmark provider |
| `sendgrid` | no | SendGrid provider |
| `smtp` | no | SMTP provider (any server, via lettre) |

The two Lettermint backends are mutually exclusive; enabling both is a compile error. To use reqwest 0.12, disable defaults:

```toml
polymail = { version = "0.1", default-features = false, features = ["lettermint-reqwest-012"] }
```

Enable multiple providers at once:

```toml
polymail = { version = "0.1", features = ["postmark"] }
```

## Provider capabilities

| Capability | Lettermint | Postmark | SendGrid | SMTP |
|---|---|---|---|---|
| Single send | yes | yes | yes | yes |
| Batch send (native) | yes (up to 500) | yes (up to 500) | no (sequential fallback) | no (sequential fallback) |
| Attachments | yes | yes | yes | yes |
| Inline attachments | yes | yes | yes | yes |
| Custom headers | yes | yes | yes (per-personalization) | yes |
| Multiple reply-to | yes | first only | first only | first only |
| Tags | first tag | first tag | multiple (categories) | - |
| Metadata | yes | yes | yes (as custom args) | - |

SMTP has no side channel for tags or metadata: anything added would become recipient-visible headers logged by every relay in between, so both are dropped. Explicit `.header(...)` values are still sent.

Batch size for SMTP is unbounded by polymail (sends are sequential over one pooled connection), but real servers may throttle or greylist after N messages per session.

The `smtp` feature uses rustls with bundled webpki-roots, not the system trust store. For corporate or system CAs, build your own `AsyncSmtpTransport` and pass it to `SmtpMailer::with_transport`.

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

| `SendError` | Postmark | Lettermint | SendGrid | SMTP |
|---|---|---|---|---|
| `Authentication` | — | HTTP 401/403 | HTTP 401/403 | reply 535/530/534/538/454 |
| `InvalidAddress` | error code 300 | HTTP 422 (validation) | HTTP 400 | local address parse failure |
| `InactiveRecipient` | error code 406 | batch status | — | — |
| `SpamComplaint` | error code 409 | batch status | — | — |
| `HardBounce` | error code 422 | batch status | — | reply 550/551/553 |
| `RateLimitExceeded` | error code 429 | HTTP 429 | HTTP 429 | — |
| `ServiceUnavailable` | error codes 500–504 | HTTP 5xx | HTTP 500–504 | other 4xx replies |
| `Provider` | transport errors | transport/parse errors | transport/parse errors | connection/TLS/timeout |
| `Api` | other error codes | other HTTP errors | other HTTP errors | other 5xx replies |
| `Serialization` | — | — | — | bad base64 / invalid header name |

## Testing

```sh
cargo test --all-features
```

The SMTP provider also has integration tests that send through a real SMTP
server and assert delivery via its API. They are `#[ignore]`-d by default; CI
runs them against a [mailcrab](https://github.com/tweedegolf/mailcrab) service
container. To run them locally, start mailcrab and point the tests at it:

```sh
docker run --rm -p 1025:1025 -p 1080:1080 marlonb/mailcrab
cargo test --features smtp -- --ignored
```

Override the defaults with `POLYMAIL_SMTP_HOST` / `POLYMAIL_SMTP_PORT` (SMTP) and
`POLYMAIL_MAILCRAB_API` (mailcrab HTTP API base) if the server is elsewhere.

## License

Dual-licensed under MIT or Apache 2.0.
