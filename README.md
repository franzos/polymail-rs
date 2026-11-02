# Polymail

[![ci](https://github.com/franzos/polymail-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/franzos/polymail-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/polymail.svg)](https://crates.io/crates/polymail)
[![Documentation](https://docs.rs/polymail/badge.svg)](https://docs.rs/polymail)

Unified email sending interface for Rust. Write your email once, send it through any supported provider — swap providers by changing one line.

Currently supported: [Lettermint](https://lettermint.co), [Postmark](https://postmarkapp.com), [SendGrid](https://sendgrid.com), [Amazon SES](https://aws.amazon.com/ses/), and any SMTP server (via [lettre](https://github.com/lettre/lettre)).

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

### Amazon SES

The `ses` feature sends through the SESv2 `SendEmail` API with the official AWS SDK. The message goes out as raw MIME, so attachments, inline images and custom headers behave the same as SMTP, and SES's message ID comes back in `SendResult`.

```rust,ignore
use polymail::provider::ses::SesMailer;

// Region and credentials from the AWS default chain:
// env, profile, SSO, web identity (EKS), ECS task role, EC2 instance profile.
let mailer = SesMailer::from_env().await;

// Fixed region, credentials still from the default chain (no async needed):
let mailer = SesMailer::with_default_credentials("eu-central-1");

// Static access keys:
let mailer = SesMailer::new("eu-central-1", "AKIA...", "secret")
    .configuration_set("tracking");
```

For retries, timeouts, a custom endpoint or anything else the SDK exposes, build an `aws_sdk_sesv2::Client` yourself and pass it to `SesMailer::with_client`. The SDK already retries throttling and 5xx responses before polymail sees an error.

`metadata` becomes SES message tags (SES only allows ASCII letters, digits, `_` and `-` in tag names and values, up to 256 characters, and rejects the send otherwise). `tags` has no SES equivalent and is dropped.

The feature pulls in the AWS SDK and `aws-lc-rs`, which compiles C code, so building it needs a C compiler.

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

### Config-driven setup

Most apps compile in several providers but let the operator pick one at runtime. The `config` feature adds `ProviderConfig`, a `serde`-deserializable schema you embed in your own config (loaded from env, TOML, wherever). Polymail doesn't read env or files itself; you deserialize, then call `build()`.

```toml
polymail = { version = "0.1", features = ["config", "postmark", "smtp"] }
```

A single provider is one table, internally tagged on `provider`:

```toml
[mail]
provider = "lettermint"
token = "your-api-token"
```

```rust,ignore
use polymail::{ProviderConfig, Mailer};

// `cfg` came from your app's own config loading (figment, config, serde, ...).
let cfg: ProviderConfig = /* ... */;
let mailer: Box<dyn Mailer> = cfg.build()?;
```

Per provider, the keys are: `lettermint` → `token`; `postmark` → `token`; `sendgrid` → `api_key`; `ses` → `region`, optional `access_key_id` / `secret_access_key` (both or neither; without them, the AWS default credential chain), optional `configuration_set`; `smtp` → `host`, optional `port` (defaults to 465 for `implicit`, 587 for `start_tls`), `tls` (`implicit` (default) / `start_tls` / `none`), optional `user` / `pass`.

Only providers you compiled in are valid variants. A config naming one you left out fails to deserialize with an "unknown variant" error listing the ones that are available.

For the exotic fallback case, it's the same type in a list. `FallbackMailer::from_configs` builds the chain in order:

```toml
[[mail]]
provider = "lettermint"
token = "your-api-token"

[[mail]]
provider = "smtp"
host = "smtp.example.com"
port = 587
tls = "start_tls"
user = "user"
pass = "pass"
```

```rust,ignore
use polymail::{FallbackMailer, ProviderConfig};

// `configs: Vec<ProviderConfig>` from your loader.
let mailer = FallbackMailer::from_configs(configs)?;
```

Note: building an SMTP mailer (directly or via config) sets up a lettre connection pool and must run inside a Tokio runtime.

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `lettermint-reqwest-013` | yes | Lettermint provider on reqwest 0.13 (rustls) |
| `lettermint-reqwest-012` | no | Lettermint provider on reqwest 0.12 (rustls) |
| `lettermint` | no | Lettermint provider without a reqwest backend (pick one of the two above) |
| `postmark` | no | Postmark provider |
| `sendgrid` | no | SendGrid provider |
| `smtp` | no | SMTP provider (any server, via lettre) |
| `ses` | no | Amazon SES provider (SESv2 API, via the AWS SDK) |
| `config` | no | Deserializable `ProviderConfig` for TOML/env-driven setup (pulls in serde) |

The two Lettermint backends are mutually exclusive; enabling both is a compile error. To use reqwest 0.12, disable defaults:

```toml
polymail = { version = "0.1", default-features = false, features = ["lettermint-reqwest-012"] }
```

Enable multiple providers at once:

```toml
polymail = { version = "0.1", features = ["postmark"] }
```

## Provider capabilities

| Capability | Lettermint | Postmark | SendGrid | SES | SMTP |
|---|---|---|---|---|---|
| Single send | yes | yes | yes | yes | yes |
| Batch send (native) | yes (up to 500) | yes (up to 500) | no (sequential fallback) | no (sequential fallback) | no (sequential fallback) |
| Attachments | yes | yes | yes | yes | yes |
| Inline attachments | yes | yes | yes | yes | yes |
| Custom headers | yes | yes | yes (per-personalization) | yes | yes |
| Multiple reply-to | yes | first only | first only | first only | first only |
| Tags | first tag | first tag | multiple (categories) | - | - |
| Metadata | yes | yes | yes (as custom args) | yes (as message tags) | - |

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

| `SendError` | Postmark | Lettermint | SendGrid | SES | SMTP |
|---|---|---|---|---|---|
| `Authentication` | — | HTTP 401/403 | HTTP 401/403 | HTTP 401/403 | reply 535/530/534/538/454 |
| `InvalidAddress` | error code 300 | HTTP 422 (validation) | HTTP 400 | `BadRequestException`, local address parse failure | local address parse failure |
| `InactiveRecipient` | error code 406 | batch status | — | — | — |
| `SpamComplaint` | error code 409 | batch status | — | — | — |
| `HardBounce` | error code 422 | batch status | — | — | reply 550/551/553 |
| `RateLimitExceeded` | error code 429 | HTTP 429 | HTTP 429 | `TooManyRequestsException`, `LimitExceededException` (quota) | — |
| `ServiceUnavailable` | error codes 500–504 | HTTP 5xx | HTTP 500–504 | HTTP 5xx, account suspended, sending paused | other 4xx replies |
| `Provider` | transport errors | transport/parse errors | transport/parse errors | transport/credential errors | connection/TLS/timeout |
| `Api` | other error codes | other HTTP errors | other HTTP errors | `MessageRejected`, unverified MAIL FROM, unknown configuration set, other HTTP errors | other 5xx replies |
| `Serialization` | — | — | — | bad base64 / invalid header name | bad base64 / invalid header name |

## Testing

The two Lettermint backends are mutually exclusive, so `--all-features` won't compile; test one backend at a time, alongside the other providers and `config`:

```sh
cargo test --no-default-features --features lettermint-reqwest-013,postmark,sendgrid,smtp,ses,config
cargo test --no-default-features --features lettermint-reqwest-012,postmark,sendgrid,smtp,ses,config
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

The SES unit tests run against a captured HTTP client, with no AWS account needed. One `#[ignore]`-d test sends a real email; it needs credentials in the AWS default chain plus `POLYMAIL_SES_FROM` / `POLYMAIL_SES_TO` (both verified identities while the account is in the SES sandbox):

```sh
POLYMAIL_SES_FROM=sender@example.com POLYMAIL_SES_TO=you@example.com \
  cargo test --features ses --test ses_integration -- --ignored
```

## License

Dual-licensed under MIT or Apache 2.0.
