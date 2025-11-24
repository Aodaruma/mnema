use crate::ids::{LlmMemoryId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmMemory {
    pub id: LlmMemoryId,
    pub user_id: UserId,
    pub content: String,
    pub structured_preferences: Option<Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
