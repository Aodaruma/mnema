use crate::ids::{AssistantId, LlmMemoryId};
use crate::user_settings::AutomationFeature;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AvatarKind {
    Static,
    Gif,
    Webm,
    Live2d,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assistant {
    pub id: AssistantId,
    pub name: String,
    pub persona_prompt: String,
    pub avatar_kind: AvatarKind,
    pub avatar_path: Option<String>,
    pub enabled_features: Vec<AutomationFeature>,
    pub memory_id: Option<LlmMemoryId>,
}
