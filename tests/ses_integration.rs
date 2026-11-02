//! Live SES send, ignored by default. Needs AWS credentials in the default
//! chain plus `POLYMAIL_SES_FROM` and `POLYMAIL_SES_TO` (both verified
//! identities while the account is in the SES sandbox).
#![cfg(feature = "ses")]

use polymail::provider::ses::SesMailer;
use polymail::{Body, Email, Mailer};

#[tokio::test]
#[ignore = "sends a real email through SES"]
async fn sends_through_ses() {
    let from = std::env::var("POLYMAIL_SES_FROM").expect("POLYMAIL_SES_FROM");
    let to = std::env::var("POLYMAIL_SES_TO").expect("POLYMAIL_SES_TO");
    let mailer = SesMailer::from_env().await;

    let email = Email::builder(
        from,
        "polymail SES test",
        Body::Text("Sent by polymail.".into()),
    )
    .to(to)
    .metadata("source", "polymail-test")
    .build()
    .unwrap();

    let result = mailer.send(&email).await.unwrap();
    assert!(result.message_id.is_some());
}
