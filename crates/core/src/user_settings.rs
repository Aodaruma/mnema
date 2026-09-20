use crate::ids::UserId;
use serde::{Deserialize, Serialize};
use time::Time;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutomationLevel {
    Off,
    #[default]
    Ask,
    AutoWithReview,
    AutoSilent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutomationFeature {
    InboxClassify,
    DueSuggestion,
    ScheduleGeneration,
    WeeklyReview,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationSettings {
    pub inbox_classify: AutomationLevel,
    pub due_suggestion: AutomationLevel,
    pub schedule_generation: AutomationLevel,
    pub weekly_review: AutomationLevel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklyReviewSettings {
    pub scheduled_time: Option<Time>,
    pub template: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    #[default]
    Local,
    OpenAiCompatible,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSettings {
    pub user_id: UserId,
    pub provider: LlmProvider,
    pub model_for_planning: Option<String>,
    pub model_for_routine: Option<String>,
    pub automation: AutomationSettings,
    pub weekly_review: WeeklyReviewSettings,
}

impl Default for UserSettings {
    fn default() -> Self {
        UserSettings {
            user_id: UserId::new(),
            provider: LlmProvider::default(),
            model_for_planning: None,
            model_for_routine: None,
            automation: AutomationSettings::default(),
            weekly_review: WeeklyReviewSettings::default(),
        }
    }
}
