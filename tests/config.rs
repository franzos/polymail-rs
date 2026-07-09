#![cfg(feature = "config")]

// `ProviderConfig` just derives `Deserialize`; the app owns loading. These
// tests use serde_json (already a dev-dep) purely to exercise that wiring —
// any serde format (toml, yaml, env via figment/config) works the same way.

use polymail::ProviderConfig;
use serde_json::json;

#[cfg(feature = "lettermint")]
#[test]
fn lettermint_deserializes_and_builds() {
    let cfg: ProviderConfig =
        serde_json::from_value(json!({ "provider": "lettermint", "token": "lm-token" })).unwrap();
    assert!(matches!(cfg, ProviderConfig::Lettermint { .. }));
    cfg.build().unwrap();
}

#[cfg(feature = "postmark")]
#[test]
fn postmark_deserializes_and_builds() {
    let cfg: ProviderConfig =
        serde_json::from_value(json!({ "provider": "postmark", "token": "pm-token" })).unwrap();
    cfg.build().unwrap();
}

#[cfg(feature = "sendgrid")]
#[test]
fn sendgrid_deserializes_and_builds() {
    let cfg: ProviderConfig =
        serde_json::from_value(json!({ "provider": "sendgrid", "api_key": "sg-key" })).unwrap();
    cfg.build().unwrap();
}

#[cfg(feature = "smtp")]
#[tokio::test]
async fn smtp_deserializes_and_builds_full() {
    let cfg: ProviderConfig = serde_json::from_value(json!({
        "provider": "smtp",
        "host": "smtp.example.com",
        "port": 587,
        "tls": "start_tls",
        "user": "user",
        "pass": "pass",
    }))
    .unwrap();
    cfg.build().unwrap();
}

#[cfg(feature = "smtp")]
#[tokio::test]
async fn smtp_tls_defaults_to_implicit() {
    // No `tls` key: defaults to implicit, and no port is fine (provider default).
    let cfg: ProviderConfig =
        serde_json::from_value(json!({ "provider": "smtp", "host": "smtp.example.com" })).unwrap();
    cfg.build().unwrap();
}

#[test]
fn unknown_provider_is_rejected() {
    let err = serde_json::from_value::<ProviderConfig>(
        json!({ "provider": "carrier-pigeon", "token": "nope" }),
    )
    .unwrap_err();
    assert!(err.to_string().contains("carrier-pigeon"));
}

#[cfg(all(feature = "lettermint", feature = "smtp"))]
#[tokio::test]
async fn fallback_chain_from_configs() {
    use polymail::FallbackMailer;

    let configs: Vec<ProviderConfig> = serde_json::from_value(json!([
        { "provider": "lettermint", "token": "lm-token" },
        {
            "provider": "smtp",
            "host": "smtp.example.com",
            "port": 587,
            "tls": "start_tls",
            "user": "user",
            "pass": "pass",
        },
    ]))
    .unwrap();

    assert_eq!(configs.len(), 2);
    FallbackMailer::from_configs(configs).unwrap();
}
