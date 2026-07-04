//! SMTP integration tests against a live mailcrab sink.
//!
//! Ignored by default (no server on a dev machine). CI starts a mailcrab
//! service container and runs them with `--ignored`. Point them at another
//! server via `POLYMAIL_SMTP_HOST` / `POLYMAIL_SMTP_PORT` (SMTP) and
//! `POLYMAIL_MAILCRAB_API` (mailcrab HTTP API base).
//!
//! mailcrab has no search or per-message delete, so tests isolate themselves by
//! a unique subject and assert on `>=` counts rather than clearing state.
#![cfg(feature = "smtp")]

use std::time::Duration;

use polymail::provider::smtp::SmtpMailer;
use polymail::{Address, Attachment, BatchItemResult, Body, Email, Mailer};
use serde_json::Value;

fn smtp_host() -> String {
    std::env::var("POLYMAIL_SMTP_HOST").unwrap_or_else(|_| "localhost".into())
}

fn smtp_port() -> u16 {
    std::env::var("POLYMAIL_SMTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(1025)
}

fn api() -> String {
    std::env::var("POLYMAIL_MAILCRAB_API").unwrap_or_else(|_| "http://localhost:1080".into())
}

fn mailer() -> SmtpMailer {
    SmtpMailer::plaintext(smtp_host(), smtp_port())
}

async fn messages_with_subject(client: &reqwest::Client, subject: &str) -> Vec<Value> {
    let all: Value = client
        .get(format!("{}/api/messages", api()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    all.as_array()
        .map(|msgs| {
            msgs.iter()
                .filter(|m| m["subject"] == subject)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

async fn wait_for(client: &reqwest::Client, subject: &str, want: usize) -> Vec<Value> {
    for _ in 0..50 {
        let msgs = messages_with_subject(client, subject).await;
        if msgs.len() >= want {
            return msgs;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {want} message(s) with subject {subject}");
}

async fn full_message(client: &reqwest::Client, id: &str) -> Value {
    client
        .get(format!("{}/api/message/{id}", api()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn attachment_text(client: &reqwest::Client, id: &str, index: usize) -> String {
    client
        .get(format!("{}/api/message/{id}/attachment/{index}", api()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

async fn raw_source(client: &reqwest::Client, id: &str) -> String {
    client
        .get(format!("{}/api/message/{id}/raw", api()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires a running mailcrab (provided by CI); run with --ignored"]
async fn delivers_plain_text() {
    let client = reqwest::Client::new();
    let subject = "polymailittext";

    let email = Email::builder(
        Address::new("sender@example.com"),
        subject,
        Body::Text("integration plain body".into()),
    )
    .to("recipient@example.com")
    .build()
    .unwrap();

    let res = mailer().send(&email).await.unwrap();
    assert!(res.message_id.is_none());

    let msgs = wait_for(&client, subject, 1).await;
    let id = msgs[0]["id"].as_str().unwrap();
    let msg = full_message(&client, id).await;

    assert_eq!(msg["from"]["email"], "sender@example.com");
    assert_eq!(msg["to"][0]["email"], "recipient@example.com");
    assert_eq!(msg["subject"], subject);
    assert!(
        msg["text"]
            .as_str()
            .unwrap()
            .contains("integration plain body")
    );
}

#[tokio::test]
#[ignore = "requires a running mailcrab (provided by CI); run with --ignored"]
async fn delivers_html_and_text() {
    let client = reqwest::Client::new();
    let subject = "polymailitboth";

    let email = Email::builder(
        Address::new("sender@example.com"),
        subject,
        Body::Both {
            html: "<p>rich integration</p>".into(),
            text: "poor integration".into(),
        },
    )
    .to("recipient@example.com")
    .build()
    .unwrap();

    mailer().send(&email).await.unwrap();

    let msgs = wait_for(&client, subject, 1).await;
    let id = msgs[0]["id"].as_str().unwrap();
    let msg = full_message(&client, id).await;

    assert!(
        msg["html"]
            .as_str()
            .unwrap()
            .contains("<p>rich integration</p>")
    );
    assert!(msg["text"].as_str().unwrap().contains("poor integration"));
}

#[tokio::test]
#[ignore = "requires a running mailcrab (provided by CI); run with --ignored"]
async fn delivers_attachment_decoded() {
    let client = reqwest::Client::new();
    let subject = "polymailitattach";

    // "YXR0LWJvZHk=" is base64 for the raw bytes "att-body".
    let email = Email::builder(
        Address::new("sender@example.com"),
        subject,
        Body::Text("see attachment".into()),
    )
    .to("recipient@example.com")
    .attachment(Attachment {
        filename: "invoice.txt".into(),
        content: "YXR0LWJvZHk=".into(),
        content_type: "text/plain".into(),
        content_id: None,
    })
    .build()
    .unwrap();

    mailer().send(&email).await.unwrap();

    let msgs = wait_for(&client, subject, 1).await;
    let id = msgs[0]["id"].as_str().unwrap();
    let msg = full_message(&client, id).await;

    let atts = msg["attachments"].as_array().unwrap();
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0]["filename"], "invoice.txt");

    // Fetch the decoded attachment: proves we did not double-encode the base64 input.
    assert_eq!(attachment_text(&client, id, 0).await, "att-body");
}

#[tokio::test]
#[ignore = "requires a running mailcrab (provided by CI); run with --ignored"]
async fn does_not_leak_tags_or_metadata() {
    let client = reqwest::Client::new();
    let subject = "polymailitheaders";

    let email = Email::builder(
        Address::new("sender@example.com"),
        subject,
        Body::Text("header check".into()),
    )
    .to("recipient@example.com")
    .header("X-Custom", "customval123")
    .tag("leaktagxyz")
    .metadata("leakkeyxyz", "leakvalxyz")
    .build()
    .unwrap();

    mailer().send(&email).await.unwrap();

    let msgs = wait_for(&client, subject, 1).await;
    let id = msgs[0]["id"].as_str().unwrap();
    let raw = raw_source(&client, id).await;

    assert!(raw.contains("X-Custom: customval123"));
    assert!(!raw.contains("leaktagxyz"));
    assert!(!raw.contains("leakkeyxyz"));
    assert!(!raw.contains("leakvalxyz"));
}

#[tokio::test]
#[ignore = "requires a running mailcrab (provided by CI); run with --ignored"]
async fn batch_delivers_all() {
    let client = reqwest::Client::new();
    let subject = "polymailitbatch";

    let emails: Vec<Email> = (0..3)
        .map(|i| {
            Email::builder(
                Address::new("sender@example.com"),
                subject,
                Body::Text(format!("batch body {i}")),
            )
            .to("recipient@example.com")
            .build()
            .unwrap()
        })
        .collect();

    let results = mailer().batch_send(&emails).await.unwrap();
    assert_eq!(results.len(), 3);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, BatchItemResult::Success(_)))
    );

    let msgs = wait_for(&client, subject, 3).await;
    assert!(msgs.len() >= 3);
}
