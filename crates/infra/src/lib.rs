// crates/infra/src/lib.rs

//! Infrastructure layer for Mnema.
//!
//! Database, LLM clients, job queue, etc.

pub mod prelude {
    // Re-export infra helpers later.
    pub use crate::calendar::{
        CalendarReadApi, CalendarWriteApi, CredentialStore, EncryptedFileCredentialStore,
        GoogleCalendarAdapter, GoogleCalendarConfig, KeyringCredentialStore, OAuthPkce,
    };
    pub use crate::db::Vault;
    pub use crate::llm::{
        ChatMessage, ChatRole, LlmClient, LlmConfig, OllamaClient, OpenAiCompatibleClient,
        factory::LlmFactory,
    };
}

pub mod automation;
pub mod calendar;
pub mod db;
pub mod llm;
