mod email;
mod error;
mod fallback;
mod mailer;
pub mod provider;

pub use email::{Address, Attachment, Body, Email};
pub use error::SendError;
pub use fallback::FallbackMailer;
pub use mailer::{BatchItemResult, Mailer, SendResult};
