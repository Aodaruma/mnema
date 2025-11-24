// crates/infra/src/lib.rs

//! Infrastructure layer for Mnema.
//!
//! Database, LLM clients, job queue, etc.

pub mod prelude {
    // Re-export infra helpers later.
    pub use crate::db::Vault;
    pub use crate::llm::{
        ChatMessage, ChatRole, LlmClient, LlmConfig, OllamaClient, OpenAiCompatibleClient,
    };
}

pub mod db;
pub mod llm;
