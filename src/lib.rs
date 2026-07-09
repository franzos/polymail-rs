#[cfg(feature = "config")]
mod config;
mod email;
mod error;
mod fallback;
mod mailer;
pub mod provider;

#[cfg(feature = "config")]
pub use config::ProviderConfig;
pub use email::{Address, Attachment, Body, Email};
pub use error::SendError;
pub use fallback::FallbackMailer;
pub use mailer::{BatchItemResult, Mailer, SendResult};
